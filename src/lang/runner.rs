use super::java::JavaRunner;
use crate::{config::CONFIG, executable::Language, unpacker::find_in_dir};
use async_trait::async_trait;
use log::{debug, error, warn};
#[cfg(unix)]
use nix::sys::signal::Signal;
use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
    process::ExitStatus,
    time,
};
use tokio::process::ChildStdout;
use tokio::{fs::copy, io};

#[derive(Debug)]
#[non_exhaustive]
pub struct Error {
    pub description: String,
}
impl Error {
    #[inline]
    pub fn new(description: &'static str) -> Self {
        Self {
            description: description.into(),
        }
    }
}
impl Display for Error {
    #[inline]
    fn fmt(&self, f: &mut Formatter) -> Result<(), core::fmt::Error> {
        write!(f, "error: {}", self.description)
    }
}

pub async fn from_dir(p: PathBuf, lang: Option<Language>) -> Option<Box<dyn Runner>> {
    //probe
    if lang.is_some() && lang.unwrap() != Language::Java {
        error!("Language other than java not yet implemented!");
        return None;
    }
    for i in &CONFIG.dependencies {
        if copy(i, p.clone().join(i.file_name().unwrap()))
            .await
            .is_err()
        {
            error!("Failed to copy dependency: {i:?}");
        }
    }
    let entry = match find_in_dir(&p, &CONFIG.entry)
        .or_else(|| find_in_dir(&p, &CONFIG.entry.clone().to_lowercase()))
    {
        Some(s) => s,
        None => {
            warn!("Failed to find entry point! Falling back to \"Main\".");
            match find_in_dir(&p, "main").or_else(|| find_in_dir(&p, "Main")) {
                Some(s) => s,
                None => {
                    error!("Failed to find main!");
                    if p.read_dir().unwrap().count() > 1 {
                        error!("There are too many files! Failed to determine which one to use!");
                        return None;
                    }
                    warn!("Will run any file inside target directory.");
                    p.read_dir()
                        .unwrap()
                        .next()
                        .unwrap()
                        .ok()
                        .map(|el| el.path())
                        .unwrap()
                }
            }
        }
    };
    debug!("Finished probing. Entry point: {entry:?}");
    match entry.extension().unwrap().to_str().unwrap() {
        "java" => Some(Box::new(JavaRunner::new_from_venv(p, entry).await.unwrap())),
        ext => {
            error!("Unknown extension: {ext}");
            None
        }
    }
}

impl core::error::Error for Error {}

#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    CE(Option<i32>, String),
    RE(Option<i32>, String),
}

#[async_trait]
pub trait Runner: Send + Sync {
    async fn prepare(&mut self) -> Result<(), RunError>;
    async fn new_from_venv(p: PathBuf, entry: PathBuf) -> Result<Self, Error>
    where
        Self: Sized;
    async fn running(&mut self) -> bool;
    async fn run(&mut self) -> Result<(), RunError>;
    async fn get_lang(&self) -> crate::executable::Language;
    async fn stdin(&mut self, s: String) -> Result<(), String>;
    async fn stdout(&mut self) -> Option<&mut ChildStdout>;
    async fn read_all(&mut self) -> Result<String, String>;
    async fn runtime(&self) -> Result<time::Duration, ()>;
    #[cfg(unix)]
    async fn signal(&mut self, s: Signal) -> Result<(), String>;
    async fn exitcode(&mut self) -> Result<Option<ExitStatus>, std::io::Error>;
    async fn add_dep(&mut self, p: PathBuf) -> Result<(), String>;
    async fn add_deps(&mut self, p: Vec<PathBuf>) -> Result<(), String>;
    async fn wait(&mut self) -> io::Result<ExitStatus>;
}
