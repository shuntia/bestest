use console::style;
use imara_diff::{diff, intern::InternedInput, Algorithm};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use log::{debug, error, info};
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, ops::Range, path::PathBuf, time::Duration};
use tokio::sync::Semaphore;

use crate::config::MULTIPROG;
use crate::{
    config::{self, CONFIG},
    executable::Executable,
};
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

#[derive(Debug)]
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
}

#[derive(Debug)]
pub struct WrongLine<T> {
    before: Option<Range<T>>,
    after: Range<T>,
}

pub async fn test_dirs<T: IntoIterator<Item = PathBuf>>(
    p: T,
) -> Vec<Result<Vec<TestResult<usize>>, String>> {
    let max_threads = config::get_config().unwrap().threads;
    let semaphore = Arc::new(Semaphore::new(max_threads as usize));
    let mut handles = vec![];
    let mp = MULTIPROG.lock().unwrap();
    if mp.clear().is_err() {
        error!("failed to clear multiprog instance!");
    }
    let mut it = p.into_iter();
    let mut v = vec![];
    let op = mp.add(ProgressBar::new(it.by_ref().count() as u64));
    for i in it {
        v.push(i.clone());
        handles.push(tokio::task::spawn(test_file_semaphore(
            i,
            semaphore.clone(),
        )));
    }
    debug!("Processing: {:#?}", v);
    let mut ret = vec![];
    let mut acc = 0;
    for i in handles {
        ret.push(i.await.unwrap());
        match ret.get(acc).unwrap() {
            Ok(o) => info!(
                "Completed {} with result: {:?}",
                v.get(acc).unwrap().file_name().unwrap().to_str().unwrap(),
                o
            ),
            Err(e) => info!("Program errored out: {}", e),
        }
        acc += 1;
        op.inc(1);
    }
    return ret;
}

pub async fn test_file_semaphore(
    path: PathBuf,
    semaphore: Arc<Semaphore>,
) -> Result<Vec<TestResult<usize>>, String> {
    let permit = semaphore.acquire().await.unwrap();
    let ret = test_file(path).await;
    drop(permit);
    ret
}

async fn test_file_semaphore_prog(
    path: PathBuf,
    semaphore: Arc<Semaphore>,
) -> Result<Vec<TestResult<usize>>, String> {
    let mut mp = MULTIPROG.lock().unwrap();
    let prog = mp.add(ProgressBar::new_spinner());
    prog.clone()
        .with_style(
            ProgressStyle::default_spinner()
                .template("{spinner} Testing {msg}")
                .unwrap(),
        )
        .set_message(format!("{}", path.clone().to_str().unwrap()));
    let ret = test_file_semaphore(path.clone(), semaphore).await;
    prog.with_finish(ProgressFinish::WithMessage(
        match &ret {
            Ok(_) => format!(
                "{} All test have passed for {}",
                style("[AC] ").bold().green(),
                path.to_str().unwrap()
            ),
            Err(e) => format!(
                "{} Failed to run Program: {} due to {}",
                style("[ERR]").bold().red(),
                path.to_str().unwrap(),
                e
            ),
        }
        .into(),
    ));
    ret
}

pub async fn test_file(path: PathBuf) -> Result<Vec<TestResult<usize>>, String> {
    info!("testing {:?}", path);
    let timeout = config::get_config().unwrap().timeout;
    let mut exec = Executable::new(path.clone(), false);
    let mut proc;
    let expected_in = config::get_config().unwrap().input.clone();
    let expected_out = config::get_config().unwrap().output.clone();
    let result = vec![];
    for i in 0..expected_in.len() {
        proc = exec.dry_run().await.unwrap();
        let _ = proc.read_all();
        for j in expected_in.get(i).unwrap() {
            proc.stdin(j.to_string()).unwrap_or_else(|e| {
                error!(
                    "failed to input stdin for process: {}",
                    &path.to_string_lossy()
                );
                error!("Reason: {}", e)
            })
        }
        while !proc.running() {
            if proc.runtime().unwrap() > Duration::new(timeout / 1000, (timeout % 1000) as u32) {
                match proc.signal(nix::sys::signal::Signal::SIGKILL) {
                    Err(e) => error!("failed to kill process: {}", e),
                    Ok(()) => {}
                }
                info!("Waiting for kill...");
                while !proc.running() {}
                return Err("Timed out.".into());
            }
        }
        let out = proc.read_all()?;
        let input = InternedInput::new(expected_out.get(i).unwrap().as_str(), &out.as_str());
        imara_diff::diff(Algorithm::Histogram, input)
    }
    Ok(result)
}

pub fn test_proc(proc: std::process::Child) -> Result<TestResult<usize>, String> {
    if proc.stdin.is_none() || proc.stdout.is_none() {
        return Err("stdin and/or stdout is not piped correctly!".into());
    }
    todo!("change Option to Vec");
    #[allow(unreachable_code)]
    Ok(TestResult::correct())
}
