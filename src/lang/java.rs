use super::runner::{Error, RunError, Runner};
use crate::executable::Language;
use async_trait::async_trait;
use log::{debug, warn};
#[cfg(unix)]
use nix::sys::signal::{Signal, kill};
use std::{
    fs::create_dir_all,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::{
    fs::copy,
    io::{self, AsyncReadExt as _, AsyncWriteExt as _},
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

#[async_trait]
impl Runner for JavaRunner {
    async fn add_dep(&mut self, p: PathBuf) -> Result<(), String> {
        self.deps.push(p.clone());
        let target = self
            .venv
            .clone()
            .unwrap()
            .join(PathBuf::from(p.file_name().unwrap()));
        copy(p, target).await.map_err(|e| format!("{e}"))?;
        Ok(())
    }
    async fn add_deps(&mut self, p: Vec<PathBuf>) -> Result<(), String> {
        self.deps.extend(p.clone());
        let venvdir = self.venv.clone();
        create_dir_all(venvdir.clone().unwrap()).map_err(|e| format!("{e}"))?;
        for i in p {
            let target = venvdir
                .clone()
                .unwrap()
                .join(PathBuf::from(i.file_name().unwrap()));
            copy(i, target).await.map_err(|e| format!("{e}"))?;
        }
        Ok(())
    }
    async fn prepare(&mut self) -> Result<(), RunError> {
        if self.entry.extension().unwrap().to_str().unwrap() == "jar" {
            debug!(
                "Skipping compile for jar file {}",
                self.entry.to_str().unwrap()
            );
            warn!("If this file only contains .java files, this may greatly decrease efficiency.");
            Ok(())
        } else {
            let mut compiler = Command::new("javac")
                .current_dir(self.venv.clone().unwrap())
                .arg(self.entry.to_str().unwrap())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            match compiler.wait().await {
                Ok(s) => {
                    if s.code().unwrap() == 0 {
                        Ok(())
                    } else {
                        let mut r = String::new();
                        let _ = compiler.stderr.unwrap().read_to_string(&mut r).await;
                        Err(RunError::CE(Some(s.code().unwrap()), r))
                    }
                }
                Err(e) => Err(RunError::CE(None, e.to_string())),
            }
        }
    }
    async fn stdin(&mut self, input: String) -> Result<(), String> {
        match &mut self.process {
            Some(s) => s
                .stdin
                .as_mut()
                .unwrap()
                .write_all(input.as_bytes())
                .await
                .map_err(|e| format!("{e}")),
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
                debug!("detected bare java file.");
                ret = Self {
                    start: None,
                    command: Command::new("java"),
                    process: None,
                    venv: Some(venv.clone()),
                    entry: entry.clone(),
                    deps: vec![],
                    exitcode: OnceLock::new(),
                };
                ret.command
                    .arg("-cp")
                    .arg(venv.to_str().unwrap())
                    .arg(entry.file_stem().unwrap())
                    .stdin(Stdio::piped())
                    .stderr(Stdio::piped())
                    .stdout(Stdio::piped());
            }
            "jar" => {
                debug!("detected java executable archive.");
                ret = Self {
                    start: None,
                    command: Command::new("java"),
                    process: None,
                    venv: Some(venv),
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
        }
        Ok(ret)
    }
    async fn run(&mut self) -> Result<(), RunError> {
        let mut contains = false;
        for i in self.venv.as_ref().unwrap().read_dir().unwrap() {
            if i.unwrap().path().extension().unwrap().to_str().unwrap() == "class" {
                contains = true;
                break;
            }
        }
        if !contains {
            debug!("Hasn't been compiled and prepared yet! Compiling...");
            self.prepare().await?;
        }
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
    #[cfg(unix)]
    async fn signal(&mut self, s: Signal) -> Result<(), String> {
        #[cfg(unix)]
        let pid = nix::unistd::Pid::from_raw(if let Some(c) = &self.process {
            c.id().unwrap() as i32
        } else {
            log::error!("tried to kill PID that does not exist!");
            return Err("tried to kill PID that does not exist".into());
        });
        #[cfg(unix)]
        if let Err(e) = kill(pid, s) {
            log::error!("failed to kill PID {pid}! error: {e}");
            return Err(e.to_string());
        }
        Ok(())
    }
    async fn runtime(&self) -> Result<Duration, ()> {
        self.start.as_ref().map_or(Err(()), |s| Ok(s.elapsed()))
    }
    async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.process.as_mut().unwrap().wait().await
    }
}
