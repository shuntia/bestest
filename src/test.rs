use crate::config;
use crate::config::{CONFIG, MULTIPROG};
use crate::executable::Language;
use crate::lang::runner::{self, RunError, Runner};
use anyhow::{Context, Result};
use console::style;
use core::time::Duration;
use imara_diff::{Algorithm, Diff, InternedInput};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{debug, error, info, warn};
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
    Correct {
        case: &'static TestCase,
        output: String,
    },
    Error {
        reason: String,
        code: i32,
    },
    Wrong {
        case: &'static TestCase,
        output: String,
        diff: Diff,
    },
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

pub async fn test_dirs<T: IntoIterator<Item = PathBuf>>(
    p: T,
) -> Result<Vec<(PathBuf, Vec<TestResult>)>> {
    let cfg = config::get_config()?;
    let max_threads = cfg.threads.max(1);
    let semaphore = Arc::new(Semaphore::new(
        usize::try_from(max_threads).context("thread count exceeds usize range")?,
    ));
    let mut handles = vec![];
    let mp = MULTIPROG.lock().await;
    if let Err(e) = mp.clear() {
        warn!("Failed to clear progress bars: {e}");
    }
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
            Arc::clone(&semaphore),
            Arc::clone(&arcmp),
            Arc::clone(&pass),
        )));
    }
    drop(arcmp);
    debug!("Processing: {v:#?}");
    let mut ret = vec![];
    for handle in handles {
        let out = handle.await.context("Test task panicked")?;
        match out.1 {
            Err(RunError::RE(code, reason)) => {
                let code_value = code.unwrap_or(-1);
                let errors = (0..CONFIG.testcases.len())
                    .map(|_| TestResult::Error {
                        reason: reason.clone(),
                        code: code_value,
                    })
                    .collect::<Vec<_>>();
                ret.push((out.0, errors));
            }
            Err(RunError::CE(code, reason)) => {
                let code_value = code.unwrap_or(-1);
                let errors = (0..CONFIG.testcases.len())
                    .map(|_| TestResult::Error {
                        reason: reason.clone(),
                        code: code_value,
                    })
                    .collect::<Vec<_>>();
                ret.push((out.0, errors));
            }
            Ok(ok) => {
                ret.push((out.0, ok));
            }
        }
    }
    pass.lock().await.finish_and_clear();
    let total_cases: usize = ret.iter().map(|(_, results)| results.len()).sum();
    let passed_cases: usize = ret
        .iter()
        .map(|(_, results)| results.iter().filter(|case| case.is_correct()).count())
        .sum();
    let submissions_with_issues = ret
        .iter()
        .filter(|(_, results)| results.iter().any(|case| !case.is_correct()))
        .count();
    info!(
        "All tests complete: {passed_cases}/{total_cases} case(s) passed; {submissions_with_issues} submission(s) with failures."
    );
    Ok(ret)
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
    let permit = match semaphore.acquire().await {
        Ok(permit) => permit,
        Err(e) => {
            error!("Failed to acquire semaphore: {e}");
            return (
                path,
                Err(RunError::CE(None, format!("Semaphore closed: {e}"))),
            );
        }
    };
    let spinner_template = format!(
        "{{spinner}} [{{elapsed_precise}}] {} compiling {{msg}}",
        style("[WJ]").dim().bold()
    );
    let spinner_style = ProgressStyle::default_spinner()
        .template(&spinner_template)
        .unwrap_or_else(|err| {
            warn!("Failed to configure spinner progress style: {err}");
            ProgressStyle::default_spinner()
        })
        .tick_strings(&config::SPINNER);
    progress.set_style(spinner_style);
    let mut proc = match runner::from_dir(path.clone(), Some(Language::Java)).await {
        Some(s) => s,
        None => {
            progress.finish_and_clear();
            error!(
                "Failed to initialize runner for {}. Skipping.",
                path.display()
            );
            return (
                path,
                Err(RunError::CE(None, "Runner initialization failed".into())),
            );
        }
    };
    let Some(filenamestr) = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
    else {
        return (path, Err(RunError::CE(None, "Invalid filename".into())));
    };
    progress.set_message(filenamestr.clone());
    progress.enable_steady_tick(Duration::from_millis(100));
    if let Err(e) = proc.prepare().await {
        warn!(
            "{} {} Compile failed!",
            style("[CE]").bold().yellow(),
            path.display()
        );
        debug!("{e:#?}");
        return (path, Err(e));
    }
    progress.finish_and_clear();
    info!(
        "{} {} Compiled successfully!",
        style("[OK]").green().bold(),
        path.display()
    );
    let progress = mp.add(ProgressBar::new(CONFIG.testcases.len() as u64));
    let bar_style = ProgressStyle::default_bar()
        .template(
            "{spinner} [{elapsed_precise}] {msg} running tests [{wide_bar:.bold.cyan/blue}]({pos}/{len})",
        )
        .unwrap_or_else(|err| {
            warn!("Failed to configure bar progress style: {err}");
            ProgressStyle::default_bar()
        })
        .progress_chars("\u{2500}\u{25b6} ");
    progress.set_style(bar_style);
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
    info!("{} {}", print_tr_vec(&ret), path.display());
    progress.finish_and_clear();
    (path, Ok(ret))
}

pub async fn test_proc(
    path: PathBuf,
    proc: &mut Box<dyn Runner>,
    testcase: &'static TestCase,
) -> TestResult {
    let timeout = match config::get_config() {
        Ok(cfg) => cfg.timeout,
        Err(e) => {
            error!("Failed to load configuration: {e}");
            return TestResult::Error {
                code: -1,
                reason: format!("configuration error: {e}"),
            };
        }
    };
    if let Err(e) = proc.run().await {
        let (code, reason) = match e {
            RunError::CE(code, reason) | RunError::RE(code, reason) => (code.unwrap_or(-1), reason),
        };
        return TestResult::Error { code, reason };
    }
    if let Err(e) = proc.stdin(testcase.input.clone()).await {
        let reason = format!(
            "failed to input stdin for process {}: {e}",
            path.to_string_lossy()
        );
        error!("{reason}");
        return TestResult::Error { code: -1, reason };
    }
    if tokio::time::timeout(Duration::from_millis(timeout), proc.wait())
        .await
        .is_err()
    {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>");
        info!(
            "{} has been running for too long. Killing process...",
            filename
        );
        #[cfg(unix)]
        if let Err(e) = proc.signal(nix::sys::signal::Signal::SIGKILL).await {
            error!("failed to kill process: {e}")
        }
        while proc.running().await {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        return TestResult::Error {
            code: 9,
            reason: "Timed out.".into(),
        };
    }

    let out = match proc.read_all().await {
        Ok(data) => data,
        Err(e) => {
            return TestResult::Error {
                code: -1,
                reason: format!("failed to read stdout: {e}"),
            };
        }
    };
    let input = InternedInput::new(testcase.expected.as_str(), out.as_str());
    let diff = imara_diff::Diff::compute(Algorithm::Histogram, &input);
    if diff.count_additions() + diff.count_removals() == 0 {
        TestResult::Correct {
            case: testcase,
            output: out,
        }
    } else {
        TestResult::Wrong {
            case: testcase,
            output: out,
            diff,
        }
    }
}
