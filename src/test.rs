use crate::config;
use crate::config::{CONFIG, MULTIPROG};
use crate::executable::Language;
use crate::lang::runner::{self, RunError, Runner};
use console::style;
use core::time::Duration;
use imara_diff::{Algorithm, Diff, InternedInput};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard, Semaphore};
#[derive(Serialize, Deserialize, Clone, Debug)]
#[non_exhaustive]
pub struct TestCase {
    pub input: String,
    pub expected: String,
    pub points: u64,
}
impl core::fmt::Display for TestCase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Input: {}\nExpected Output: {}\nPoints: {}",
            self.input, self.expected, self.points
        )
    }
}

#[expect(clippy::module_name_repetitions)]
#[derive(Debug)]
#[non_exhaustive]
pub enum TestResult {
    Correct { case: &'static TestCase },
    Error { reason: String, code: i32 },
    Wrong { case: &'static TestCase, loc: Diff },
}

#[expect(clippy::missing_trait_methods)]
impl Clone for TestResult {
    fn clone(&self) -> Self {
        #[expect(clippy::pattern_type_mismatch)]
        match self {
            Self::Correct { case } => Self::Correct { case },
            Self::Error { reason, code } => Self::Error {
                reason: reason.clone(),
                code: *code,
            },
            #[expect(clippy::panic)]
            Self::Wrong { .. } => panic!("Tried to clone a diff."),
        }
    }
}

impl TestResult {
    pub const fn is_correct(&self) -> bool {
        match self {
            Self::Correct { .. } => true,
            Self::Wrong { .. } | Self::Error { .. } => false,
        }
    }
    #[must_use]
    pub fn msg(&self) -> String {
        if self.is_correct() {
            style("[AC]").green().bold().to_string()
        } else {
            style("[NG]").red().bold().to_string()
        }
    }
}

pub async fn test_dirs<T: IntoIterator<Item = PathBuf>>(p: T) -> Vec<(PathBuf, Vec<TestResult>)> {
    let max_threads = config::get_config().unwrap().threads;
    let semaphore = Arc::new(Semaphore::new(
        usize::try_from(max_threads).expect("REASON"),
    ));
    let mut handles = vec![];
    let mp = MULTIPROG.lock().await;
    let _ = mp.clear();
    let v = p.into_iter().collect::<Vec<PathBuf>>();
    let op = mp.add(ProgressBar::new(v.len() as u64));
    op.set_style(
        ProgressStyle::default_bar()
            .template(
                "[{elapsed_precise}] running tests... [{wide_bar:.bold.cyan/blue}] ({pos}/{len})",
            )
            .unwrap()
            .progress_chars("\u{2588}\u{e0b0}\u{2500}"),
    );
    op.enable_steady_tick(Duration::from_millis(100));
    let pass = Arc::new(Mutex::new(op));
    let arcmp = Arc::new(mp);
    for i in &v {
        handles.push(tokio::task::spawn(test_file_progress(
            i.clone(),
            Arc::<tokio::sync::Semaphore>::clone(&semaphore),
            Arc::<tokio::sync::MutexGuard<'_, indicatif::MultiProgress>>::clone(&arcmp),
            Arc::<tokio::sync::Mutex<indicatif::ProgressBar>>::clone(&pass),
        )));
    }
    drop(arcmp);
    debug!("Processing: {v:#?}");
    let mut ret = vec![];
    for i in handles {
        let out = i.await.unwrap();
        match out.1 {
            Err(RunError::RE(code, reason)) => ret.push((
                out.0,
                vec![
                    TestResult::Error {
                        reason,
                        code: code.unwrap()
                    };
                    CONFIG.testcases.len()
                ],
            )),
            Err(RunError::CE(code, reason)) => ret.push((
                out.0,
                vec![
                    TestResult::Error {
                        reason,
                        code: code.unwrap()
                    };
                    CONFIG.testcases.len()
                ],
            )),
            Ok(ok) => {
                ret.push((out.0, ok));
            }
        }
    }
    pass.lock().await.finish_and_clear();
    info!("All tests complete.");
    ret
}

#[must_use]
pub fn print_tr_vec(tr: &Vec<TestResult>) -> String {
    let mut acc = 0;
    for i in tr {
        if i.is_correct() {
            acc += 1;
        }
    }
    if acc == tr.len() {
        style(format!("[AC][{}/{}]", acc, tr.len()))
            .bold()
            .green()
            .to_string()
    } else {
        style(format!("[NG][{}/{}]", acc, tr.len()))
            .bold()
            .red()
            .to_string()
    }
}

pub async fn test_file_progress(
    path: PathBuf,
    semaphore: Arc<Semaphore>,
    mp: Arc<MutexGuard<'static, MultiProgress>>,
    op: Arc<Mutex<ProgressBar>>,
) -> (PathBuf, Result<Vec<TestResult>, RunError>) {
    let progress = mp.add(ProgressBar::new_spinner());
    let permit = semaphore.acquire().await.unwrap();
    progress.set_style(
        ProgressStyle::default_spinner()
            .template(
                format!(
                    "{{spinner}} [{{elapsed_precise}}] {} compiling {{msg}}",
                    style("[WJ]").dim().bold()
                )
                .as_str(),
            )
            .unwrap()
            .tick_strings(&config::SPINNER),
    );
    let mut proc = match runner::from_dir(path.clone(), Some(Language::Java)).await {
        Some(s) => s,
        None => return (path, Err(RunError::CE(None, "Unknown".into()))),
    };
    let file = path.clone().clone();
    let filename = file.file_name().unwrap();
    let filenamestr = filename.to_str().unwrap().to_owned();
    progress.set_message(filenamestr);
    progress.enable_steady_tick(Duration::from_millis(100));
    if let Err(e) = proc.prepare().await {
        info!(
            "{} {} Compile failed!",
            style("[CE]").bold().yellow(),
            path.to_str().unwrap()
        );
        debug!("{e:#?}");
        return (path, Err(e));
    }
    progress.finish_and_clear();
    info!(
        "{} {} Compiled successfully!",
        style("[OK]").green().bold(),
        path.to_str().unwrap()
    );
    let progress = mp.add(ProgressBar::new(CONFIG.testcases.len() as u64));
    progress.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner} [{elapsed_precise}] {msg} running tests [{wide_bar:.bold.cyan/blue}]({pos}/{len})",
            )
            .unwrap()
            .progress_chars("\u{2500}\u{25b6} "),
    );
    progress.enable_steady_tick(Duration::from_millis(50));
    let tc = &CONFIG.testcases;
    progress.set_message(style("[WJ] [0/?]").dim().bold().to_string());
    let mut ret = vec![];
    let mut correct = 0;
    for i in 0..tc.len() {
        let push = test_proc(path.clone(), &mut proc, &tc[i]).await;
        if push.is_correct() {
            correct += 1;
        }
        if correct == i + 1 {
            progress.set_message(
                style(format!("[AC] [{}/{}]", correct, tc.len()))
                    .green()
                    .bold()
                    .to_string(),
            );
        } else {
            progress.set_message(
                style(format!("[NG] [{}/{}]", correct, tc.len()))
                    .red()
                    .bold()
                    .to_string(),
            );
        }
        ret.push(push);
        progress.inc(1);
    }
    drop(permit);
    op.lock().await.inc(1);
    info!("{} {}", print_tr_vec(&ret), path.clone().to_str().unwrap());
    progress.finish_and_clear();
    (path, Ok(ret))
}

pub async fn test_proc(
    path: PathBuf,
    proc: &mut Box<dyn Runner>,
    testcase: &'static TestCase,
) -> TestResult {
    let timeout = config::get_config().unwrap().timeout;
    proc.run().await.unwrap();
    let _ = proc.read_all().await;
    proc.stdin(testcase.input.clone())
        .await
        .unwrap_or_else(|e| {
            error!(
                "failed to input stdin for process: {}",
                &path.to_string_lossy()
            );
            error!("Reason: {e}")
        });
    while proc.running().await {
        if proc.runtime().await.unwrap() > Duration::from_millis(timeout) {
            info!(
                "{} has been running for too long. Killing process...",
                path.file_name().unwrap().to_str().unwrap()
            );
            #[cfg(unix)]
            if let Err(e) = proc.signal(nix::sys::signal::Signal::SIGKILL).await {
                error!("failed to kill process: {e}")
            }
            while !proc.running().await {}
            return TestResult::Error {
                code: 9,
                reason: "Timed out.".into(),
            };
        }
    }
    let out = proc.read_all().await.unwrap();
    let input = InternedInput::new(testcase.expected.as_str(), out.as_str());
    let diff = imara_diff::Diff::compute(Algorithm::Histogram, &input);
    if diff.count_additions() + diff.count_removals() == 0 {
        TestResult::Wrong {
            case: testcase,
            loc: diff,
        }
    } else {
        TestResult::Correct { case: testcase }
    }
}
