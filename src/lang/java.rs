use super::runner::{Error, Runner};
use crate::executable::Language;
use async_trait::async_trait;
use log::info;
use nix::sys::signal::{kill, Signal};
use std::{
    fs::{copy, read_dir},
    io::{Read, Write},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdout, Command},
};

pub struct JavaRunner {
    start: Option<Instant>,
    command: Command,
    process: Option<Child>,
    venv: Option<PathBuf>,
    entry: PathBuf,
    deps: Vec<PathBuf>,
    exitcode: OnceLock<i32>,
}

impl JavaRunner {
    /// appends dependencies for execution. automatically creates a venv.
    pub async fn add_dep(&mut self, p: PathBuf) -> Result<(), String> {
        self.deps.push(p.clone());
        let mut target = self.venv.clone().unwrap();
        target.push(PathBuf::from(p.file_name().unwrap()));
        copy(p, target).map_err(|e| format!("{}", e).to_owned())?;
        Ok(())
    }
    /// appends dependencies for execution. automatically creates a venv.
    pub async fn append_deps(&mut self, p: Vec<PathBuf>) -> Result<(), String> {
        self.deps.extend(p.clone());
        let venvdir = self.venv.clone();
        std::fs::create_dir(venvdir.clone().unwrap()).map_err(|e| format!("{}", e).to_owned())?;
        for i in p {
            let mut target = venvdir.clone().unwrap();
            target.push(PathBuf::from(i.file_name().unwrap()));
            copy(i, target).map_err(|e| format!("{}", e).to_owned())?;
        }
        Ok(())
    }
}

#[async_trait]
impl Runner for JavaRunner {
    async fn stdin(&mut self, input: String) -> Result<(), String> {
        match &mut self.process {
            Some(s) => s
                .stdin
                .as_mut()
                .unwrap()
                .write_all(input.as_bytes())
                .await
                .map_err(|e| format!("{}", e))
                .map(|_| ()),
            None => Err("Process has not started yet!".into()),
        }
    }
    async fn stdout(&mut self) -> Option<&mut ChildStdout> {
        match &mut self.process {
            Some(s) => match &mut s.stdout {
                Some(t) => return Some(t),
                None => return None,
            },
            None => return None,
        };
    }
    async fn exitcode(&mut self) -> Result<Option<ExitStatus>, std::io::Error> {
        if self.running().await {
            self.process.as_mut().unwrap().try_wait()
        } else {
            Ok(None)
        }
    }
    async fn read_all(&mut self) -> Result<String, String> {
        let stdout = match &mut self.process {
            Some(s) => match &mut s.stdout {
                Some(t) => t,
                None => return Err("Stdout is not open!".into()),
            },
            None => return Err("Process is not running!".into()),
        };
        let mut buf: String = String::new();
        let _ = stdout.read_to_string(&mut buf).await;
        Ok(buf)
    }
    async fn new_from_venv(venv: PathBuf, entry: PathBuf) -> Result<Self, Error> {
        let mut ret;
        let ext: String = entry.extension().unwrap().to_string_lossy().into();
        match ext.as_str() {
            "java" => {
                info!("detected bare java file.");
                ret = JavaRunner {
                    start: None,
                    command: Command::new("java"),
                    process: None,
                    venv: None,
                    entry: entry.clone(),
                    deps: vec![],
                    exitcode: OnceLock::new(),
                };
                ret.command
                    .arg(&entry)
                    .arg(format!("--cp {}", venv.to_str().unwrap()))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped());
            }
            "jar" => {
                info!("detected java executable archive.");
                ret = JavaRunner {
                    start: None,
                    command: Command::new("java"),
                    process: None,
                    venv: None,
                    entry: entry.clone(),
                    deps: vec![],
                    exitcode: OnceLock::new(),
                };
                ret.command
                    .arg("--jar")
                    .arg(&entry)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped());
            }
            _ => panic!("give me a java file."),
        };
        Ok(ret)
    }
    async fn run(&mut self) -> Result<(), Error> {
        self.process = Some(self.command.spawn().unwrap());
        self.start = Some(Instant::now());
        Ok(())
    }
    async fn running(&mut self) -> bool {
        match &mut self.process {
            Some(s) => match s.try_wait().unwrap() {
                Some(s) => {
                    let _ = self.exitcode.set(s.code().unwrap());
                    false
                }
                None => true,
            },
            None => false,
        }
    }
    async fn get_lang(&self) -> Language {
        Language::Java
    }
    async fn signal(&mut self, s: Signal) -> Result<(), String> {
        let pid = nix::unistd::Pid::from_raw(match &self.process {
            Some(c) => c.id().unwrap() as i32,
            None => {
                log::error!("tried to kill PID that does not exist!");
                return Err("tried to kill PID that does not exist".into());
            }
        });
        match kill(pid, s) {
            Err(e) => {
                log::error!("failed to kill PID {}! error: {}", pid, e);
                return Err(e.to_string().into());
            }
            Ok(_) => {}
        }
        Ok(())
    }
    async fn runtime(&self) -> Result<Duration, ()> {
        match &self.start {
            Some(s) => Ok(s.elapsed()),
            None => Err(()),
        }
    }
}
