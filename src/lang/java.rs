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
        let venv = self
            .venv
            .clone()
            .ok_or_else(|| "Virtual environment is not initialized".to_string())?;
        let file_name = p
            .file_name()
            .ok_or_else(|| "Dependency path missing file name".to_string())?;
        let target = venv.join(file_name);
        copy(p, target).await.map_err(|e| format!("{e}"))?;
        Ok(())
    }
    async fn add_deps(&mut self, p: Vec<PathBuf>) -> Result<(), String> {
        self.deps.extend(p.clone());
        let venvdir = self
            .venv
            .clone()
            .ok_or_else(|| "Virtual environment is not initialized".to_string())?;
        create_dir_all(&venvdir).map_err(|e| format!("{e}"))?;
        for i in p {
            let file_name = i
                .file_name()
                .ok_or_else(|| "Dependency path missing file name".to_string())?;
            let target = venvdir.join(file_name);
            copy(i, target).await.map_err(|e| format!("{e}"))?;
        }
        Ok(())
    }
    async fn prepare(&mut self) -> Result<(), RunError> {
        let entry_ext = self
            .entry
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| RunError::CE(None, "Unsupported entry extension".into()))?;
        if entry_ext == "jar" {
            debug!("Skipping compile for jar file {}", self.entry.display());
            warn!("If this file only contains .java files, this may greatly decrease efficiency.");
            Ok(())
        } else {
            let venv = self
                .venv
                .as_ref()
                .ok_or_else(|| RunError::CE(None, "Virtual environment not prepared".into()))?;
            let mut compiler = Command::new("javac")
                .current_dir(venv)
                .arg(&self.entry)
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| RunError::CE(None, e.to_string()))?;
            match compiler.wait().await {
                Ok(s) => match s.code() {
                    Some(0) => Ok(()),
                    Some(code) => {
                        let mut r = String::new();
                        if let Some(stderr) = compiler.stderr.as_mut() {
                            let _ = stderr.read_to_string(&mut r).await;
                        }
                        Err(RunError::CE(Some(code), r))
                    }
                    None => Err(RunError::CE(
                        None,
                        "javac terminated without exit code".into(),
                    )),
                },
                Err(e) => Err(RunError::CE(None, e.to_string())),
            }
        }
    }
    async fn stdin(&mut self, input: String) -> Result<(), String> {
        match &mut self.process {
            Some(s) => match s.stdin.as_mut() {
                Some(stdin) => stdin
                    .write_all(input.as_bytes())
                    .await
                    .map_err(|e| format!("{e}")),
                None => Err("Process stdin is not available".into()),
            },
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
            if let Some(process) = self.process.as_mut() {
                process.try_wait()
            } else {
                Ok(None)
            }
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
        let ext = entry
            .extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| Error::new("Unsupported Java artifact"))?;
        match ext {
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
                    .arg(&venv)
                    .arg(
                        entry
                            .file_stem()
                            .ok_or_else(|| Error::new("Entry missing file stem"))?,
                    )
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
            _ => {
                return Err(Error::new("Unsupported Java artifact"));
            }
        }
        Ok(ret)
    }
    async fn run(&mut self) -> Result<(), RunError> {
        let venv = self
            .venv
            .as_ref()
            .ok_or_else(|| RunError::CE(None, "Virtual environment not prepared".into()))?;
        let mut contains = false;
        let entries = venv
            .read_dir()
            .map_err(|e| RunError::CE(None, e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| RunError::CE(None, e.to_string()))?;
            let is_class = entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map_or(false, |ext| ext.eq_ignore_ascii_case("class"));
            if is_class {
                contains = true;
                break;
            }
        }
        if !contains {
            debug!("Hasn't been compiled and prepared yet! Compiling...");
            self.prepare().await?;
        }
        let child = self
            .command
            .spawn()
            .map_err(|e| RunError::RE(None, e.to_string()))?;
        self.process = Some(child);
        self.start = Some(Instant::now());
        Ok(())
    }
    async fn running(&mut self) -> bool {
        match &mut self.process {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => {
                    if let Some(code) = status.code() {
                        let _ = self.exitcode.set(code);
                    }
                    false
                }
                Ok(None) => true,
                Err(e) => {
                    warn!("Failed to poll java process: {e}");
                    false
                }
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
            c.id()
                .ok_or_else(|| "Process id is unavailable".to_string())? as i32
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
        if let Some(process) = self.process.as_mut() {
            process.wait().await
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "process is not running",
            ))
        }
    }
}
