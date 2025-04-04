use console::style;
use imara_diff::{diff, intern::InternedInput, Algorithm};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread::Thread;
use std::{collections::HashMap, ops::Range, path::PathBuf, time::Duration};
use tokio::sync::Semaphore;

use crate::config::MULTIPROG;
use crate::config::{self, CONFIG};
use crate::executable::Language;
use crate::lang::runner::{self, RunError, Runner};
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TestCase {
    pub input: String,
    pub expected: String,
    pub points: u64,
}
impl std::fmt::Display for TestCase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Input: {}\nExpected Output: {}\nPoints: {}",
            self.input, self.expected, self.points
        )
    }
}

impl TestCase {
    fn diff<'a>(
        &'a self,
        s: &'a str,
    ) -> (Vec<&'a str>, Vec<&'a str>, (Vec<&'a str>, Vec<&'a str>)) {
        let mut removals = Vec::new();
        let mut insertions = Vec::new();
        let mut replacements = Vec::new();
        let input = InternedInput::new(self.expected.as_str(), s);
        let sink = |before: Range<u32>, after: Range<u32>| {
            let hunk_before: Vec<_> = input.before[before.start as usize..before.end as usize]
                .iter()
                .map(|&line| input.interner[line])
                .collect();
            let hunk_after: Vec<_> = input.after[after.start as usize..after.end as usize]
                .iter()
                .map(|&line| input.interner[line])
                .collect();
            if hunk_after.is_empty() {
                removals.push(hunk_before)
            } else if hunk_before.is_empty() {
                insertions.push(hunk_after)
            } else {
                replacements.push((hunk_before, hunk_after))
            }
        };
        diff(Algorithm::Histogram, &input, sink);
        return (
            removals[0].clone(),
            insertions[0].clone(),
            replacements[0].clone(),
        );
    }
}

const CHEAT_ENABLED: [&'static str; 2] = ["kartik", "shunta"];

#[derive(Debug, Clone)]
pub enum TestResult {
    Correct {
        case: &'static TestCase,
    },
    Wrong {
        case: &'static TestCase,
        loc: Vec<WrongLine<usize>>,
    },
    Error {
        reason: String,
        code: i32,
    },
}

impl TestResult {
    pub fn is_correct(&self) -> bool {
        match self {
            Self::Correct { .. } => true,
            _ => false,
        }
    }
    pub fn get_loc(&self) -> Option<&Vec<WrongLine<usize>>> {
        match &self {
            Self::Wrong { case, loc } => Some(loc),
            _ => None,
        }
    }
    pub fn msg(&self) -> String {
        if self.is_correct() {
            style("[AC]").green().bold().to_string()
        } else {
            style("[NG]").red().bold().to_string()
        }
    }
}

#[derive(Debug, Clone)]
pub struct WrongLine<T> {
    before: Range<T>,
    after: (Range<T>, String),
}

pub async fn test_dirs<T: IntoIterator<Item = PathBuf>>(p: T) -> Vec<(PathBuf, Vec<TestResult>)> {
    let max_threads = config::get_config().unwrap().threads;
    let semaphore = Arc::new(Semaphore::new(max_threads as usize));
    let mut handles = vec![];
    let mp = MULTIPROG.lock().unwrap();
    let _ = mp.clear();
    let v = p.into_iter().collect::<Vec<PathBuf>>();
    let op = mp.add(ProgressBar::new(v.len() as u64));
    op.enable_steady_tick(Duration::from_millis(100));
    for i in &v {}
    for i in &v {
        handles.push(tokio::task::spawn(test_file_progress(
            i.clone(),
            semaphore.clone(),
            mp.add(ProgressBar::new_spinner()),
        )));
    }
    debug!("Processing: {:#?}", v);
    let mut ret = vec![];
    for i in handles {
        let out = i.await.unwrap();
        if out.1.is_ok() {
            ret.push((out.0, out.1.unwrap()));
        }
        op.inc(1);
    }
    op.finish_and_clear();
    info!("All tests complete.");
    return ret;
}

pub async fn test_proc_semaphore(
    path: PathBuf,
    semaphore: Arc<Semaphore>,
    proc: &mut Box<dyn Runner>,
) -> Vec<TestResult> {
    let permit = semaphore.acquire().await.unwrap();
    let ret = test_proc(path.clone(), proc).await;
    drop(permit);
    info!("{} {}", print_tr_vec(&ret), path.clone().to_str().unwrap());
    ret
}

pub async fn test_file_semaphore(path: PathBuf, semaphore: Arc<Semaphore>) -> Vec<TestResult> {
    let permit = semaphore.acquire().await.unwrap();
    let ret = test_file(path.clone()).await;
    drop(permit);
    info!("{} {}", print_tr_vec(&ret), path.clone().to_str().unwrap());
    ret
}

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
    prog: ProgressBar,
) -> (PathBuf, Result<Vec<TestResult>, RunError>) {
    prog.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} compiling {msg}")
            .unwrap(),
    );
    let mut proc = match runner::from_dir(path.clone(), Some(Language::Java)).await {
        Some(s) => s,
        None => return (path, Err(RunError::CE(None, "Unknown".into()))),
    };
    let file = path.clone().to_path_buf();
    let filename = file.file_name().unwrap();
    let filenamestr = filename.to_str().unwrap().to_owned();
    prog.set_message(filenamestr);
    prog.enable_steady_tick(Duration::from_millis(50));
    match proc.prepare().await {
        Err(e) => {
            warn!(
                "{} {} Compile failed!",
                style("[CE]").bold().yellow(),
                path.to_str().unwrap()
            );
            return (path, Err(e));
        }
        Ok(_) => {}
    }
    info!(
        "{} {} Compiled successfully!",
        style("[OK]").green().bold().to_string(),
        path.to_str().unwrap()
    );
    prog.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} testing {msg}")
            .unwrap(),
    );
    let ret = test_proc_semaphore(path.clone(), semaphore, &mut Box::new(proc)).await;
    prog.finish_and_clear();
    return (path, Ok(ret));
}

pub async fn test_file(path: PathBuf) -> Vec<TestResult> {
    test_proc(
        path.clone(),
        &mut Box::new(runner::from_dir(path, Some(Language::Java)).await.unwrap()),
    )
    .await
}
pub async fn test_proc(path: PathBuf, proc: &mut Box<dyn Runner>) -> Vec<TestResult> {
    let timeout = config::get_config().unwrap().timeout;
    let testcases = &CONFIG.testcases;
    let mut ret = vec![];
    for i in 0..testcases.len() {
        let mut wrong = vec![];
        proc.run().await.unwrap();
        let _ = proc.read_all();
        proc.stdin(testcases.get(i).unwrap().input.clone())
            .await
            .unwrap_or_else(|e| {
                error!(
                    "failed to input stdin for process: {}",
                    &path.to_string_lossy()
                );
                error!("Reason: {}", e)
            });
        while proc.running().await {
            if proc.runtime().await.unwrap() > Duration::from_millis(timeout) {
                info!(
                    "{} has been running for too long. Killing process...",
                    path.file_name().unwrap().to_str().unwrap()
                );
                match proc.signal(nix::sys::signal::Signal::SIGKILL).await {
                    Err(e) => error!("failed to kill process: {}", e),
                    Ok(()) => {}
                }
                while !proc.running().await {}
                ret.push(TestResult::Error {
                    code: 9,
                    reason: "Timed out.".into(),
                });
            }
        }
        let out = proc.read_all().await.unwrap();
        let input = InternedInput::new(testcases.get(i).unwrap().expected.as_str(), &out.as_str());
        let sink = |before: Range<u32>, after: Range<u32>| {
            let hunk_after: Vec<_> = input.after[after.start as usize..after.end as usize]
                .iter()
                .map(|&line| input.interner[line])
                .collect();
            wrong.push(WrongLine::<usize> {
                before: before.start as usize..before.end as usize,
                after: (
                    after.start as usize..after.end as usize,
                    hunk_after.join("\n").to_owned(),
                ),
            });
        };
        imara_diff::diff(Algorithm::Histogram, &input, sink);
        if !wrong.is_empty() {
            ret.push(TestResult::Wrong {
                case: testcases.get(i).unwrap(),
                loc: wrong,
            });
        } else {
            ret.push(TestResult::Correct {
                case: testcases.get(i).unwrap(),
            });
        }
    }
    ret
}
