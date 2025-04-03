use console::style;
use imara_diff::{diff, intern::InternedInput, Algorithm};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use log::{debug, error, info};
use std::sync::{Arc, Mutex};
use std::thread::Thread;
use std::{collections::HashMap, ops::Range, path::PathBuf, time::Duration};
use tokio::sync::Semaphore;

use crate::config::MULTIPROG;
use crate::config::{self, CONFIG};
use crate::executable::Language;
use crate::lang::runner;
pub struct TestCase {
    input: String,
    expected: String,
    points: u64,
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
pub struct TestResult<T> {
    pub correct: bool,
    loc: Option<Vec<WrongLine<T>>>,
}

impl<T> TestResult<T> {
    pub fn is_correct(&self) -> bool {
        self.correct
    }
    pub fn get_loc(&self) -> Option<&Vec<WrongLine<T>>> {
        match &self.loc {
            Some(s) => Some(&s),
            None => None,
        }
    }
    pub fn correct() -> Self {
        Self {
            correct: true,
            loc: None,
        }
    }
    pub fn wrong(s: Vec<WrongLine<T>>) -> Self {
        Self {
            correct: false,
            loc: Some(s),
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

pub async fn test_dirs<T: IntoIterator<Item = PathBuf>>(
    p: T,
) -> Vec<Result<TestResult<usize>, String>> {
    let max_threads = config::get_config().unwrap().threads;
    let semaphore = Arc::new(Semaphore::new(max_threads as usize));
    let mut handles = vec![];
    let mp = MULTIPROG.lock().unwrap();
    let _ = mp.clear();
    let v = p.into_iter().collect::<Vec<PathBuf>>();
    let op = mp.add(ProgressBar::new(v.len() as u64));
    op.enable_steady_tick(Duration::from_millis(100));
    for i in &v {
        handles.push(tokio::task::spawn(test_file_progress(
            i.clone(),
            semaphore.clone(),
            mp.add(ProgressBar::new_spinner()),
        )));
    }
    debug!("Processing: {:#?}", v);
    let mut ret = vec![];
    let mut acc = 0;
    for i in handles {
        ret.push(i.await.unwrap());
        match ret.get(acc).unwrap() {
            Ok(o) => {
                debug!(
                    "Completed {} with result: {:?}",
                    v.get(acc).unwrap().file_name().unwrap().to_str().unwrap(),
                    o
                );
            }
            Err(e) => info!(
                "{} errored out: {}",
                v.get(acc).unwrap().file_name().unwrap().to_str().unwrap(),
                e
            ),
        }
        acc += 1;
        op.inc(1);
    }
    op.finish_and_clear();
    info!("All tests complete.");
    return ret;
}

pub async fn test_file_semaphore(
    path: PathBuf,
    semaphore: Arc<Semaphore>,
) -> Result<TestResult<usize>, String> {
    let permit = semaphore.acquire().await.unwrap();
    let ret = test_file(path.clone()).await;
    drop(permit);
    if ret.is_ok() {
        info!("{} {}", ret.clone().unwrap().msg(), path.to_str().unwrap());
    }
    ret
}

pub async fn test_file_progress(
    path: PathBuf,
    semaphore: Arc<Semaphore>,
    prog: ProgressBar,
) -> Result<TestResult<usize>, String> {
    prog.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} testing {msg}")
            .unwrap(),
    );
    let file = path.clone().to_path_buf();
    let filename = file.file_name().unwrap();
    let filenamestr = filename.to_str().unwrap().to_owned();
    prog.set_message(filenamestr);
    prog.enable_steady_tick(Duration::from_millis(50));
    let ret = test_file_semaphore(path.clone(), semaphore).await;
    prog.finish_and_clear();
    return ret;
}

pub async fn test_file(path: PathBuf) -> Result<TestResult<usize>, String> {
    info!("testing {:?}", path);
    let timeout = config::get_config().unwrap().timeout;
    let mut proc = runner::from_dir(path.clone(), Some(Language::Java))
        .await
        .unwrap();
    let expected_in = config::get_config().unwrap().input.clone();
    let expected_out = config::get_config().unwrap().output.clone();
    let mut wrong = vec![];
    for i in 0..expected_in.len() {
        proc.run().await.unwrap();
        let _ = proc.read_all();
        proc.stdin(expected_in.get(i).unwrap().clone())
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
                return Err("Timed out.".into());
            }
        }
        let out = proc.read_all().await?;
        let input = InternedInput::new(expected_out.get(i).unwrap().as_str(), &out.as_str());
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
    }

    if wrong.is_empty() {
        Ok(TestResult::correct())
    } else {
        Ok(TestResult {
            correct: false,
            loc: Some(wrong),
        })
    }
}

pub fn test_proc(proc: std::process::Child) -> Result<TestResult<usize>, String> {
    if proc.stdin.is_none() || proc.stdout.is_none() {
        return Err("stdin and/or stdout is not piped correctly!".into());
    }
    todo!("change Option to Vec");
    #[allow(unreachable_code)]
    Ok(TestResult::correct())
}
