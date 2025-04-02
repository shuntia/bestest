use std::{
    fs::{copy, read_dir},
    io::{Read, Write},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

use log::info;
use nix::sys::signal::{kill, Signal};

use crate::executable::Language;

use super::runner::{Error, Runner};

pub struct JavaRunner {
    start: Option<Instant>,
    command: Command,
    process: Option<Child>,
    venv: Option<PathBuf>,
    entry: PathBuf,
    deps: Vec<PathBuf>,
}

impl JavaRunner {
    /// appends dependencies for execution. automatically creates a venv.
    pub fn add_dep(&mut self, p: PathBuf) -> Result<(), String> {
        self.deps.push(p.clone());
        let mut target = self.venv.clone().unwrap();
        target.push(PathBuf::from(p.file_name().unwrap()));
        copy(p, target).map_err(|e| format!("{}", e).to_owned())?;
        Ok(())
    }
    /// appends dependencies for execution. automatically creates a venv.
    pub fn append_deps(&mut self, p: Vec<PathBuf>) -> Result<(), String> {
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

impl Runner for JavaRunner {
    fn stdin(&mut self, input: String) -> Result<(), String> {
        match &mut self.process {
            Some(s) => s
                .stdin
                .as_mut()
                .unwrap()
                .write_all(input.as_bytes())
                .map_err(|e| format!("{}", e))
                .map(|_| ()),
            None => Err("Process has not started yet!".into()),
        }
    }
    fn stdout(&mut self) -> Option<&mut ChildStdout> {
        match &mut self.process {
            Some(s) => match &mut s.stdout {
                Some(t) => return Some(t),
                None => return None,
            },
            None => return None,
        };
    }
    fn read_all(&mut self) -> Result<String, String> {
        let stdout = match &mut self.process {
            Some(s) => match &mut s.stdout {
                Some(t) => t,
                None => return Err("Stdout is not open!".into()),
            },
            None => return Err("Process is not running!".into()),
        };
        let mut buf: String = String::new();
        let _ = stdout.read_to_string(&mut buf);
        Ok(buf)
    }
    fn new(p: PathBuf) -> Result<JavaRunner, Error> {
        if p.is_dir() {
            let dirs = read_dir(p).unwrap();
            for i in dirs {
                let path = i.unwrap().path();
                if path.is_file() && path.file_stem().unwrap().to_string_lossy().contains("main") {
                    return Self::new(path);
                } else {
                    return Err(Error::new("unable to find entry point."));
                }
            }
            return Err(Error::new("Empty dir!"));
        } else {
            let mut ret;
            let ext: String = p.extension().unwrap().to_string_lossy().into();
            match ext.as_str() {
                "java" => {
                    info!("detected bare java file.");
                    ret = JavaRunner {
                        start: None,
                        command: Command::new("java"),
                        process: None,
                        venv: None,
                        entry: p.clone(),
                        deps: vec![],
                    };
                    ret.command
                        .arg(&p)
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
                        entry: p.clone(),
                        deps: vec![],
                    };
                    ret.command
                        .arg("--jar")
                        .arg(&p)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped());
                }
                _ => return Err(Error::new("Unknown file extension!")),
            }
            return Err(Error::new("???"));
        }
    }
    fn run(&mut self) -> Result<(), Error> {
        let _ = self.command.exec();
        self.process = Some(self.command.spawn().unwrap());
        Ok(())
    }
    fn running(&self) -> bool {
        self.process.is_some()
    }
    fn get_lang(&self) -> Language {
        Language::Java
    }
    fn signal(&mut self, s: Signal) -> Result<(), String> {
        let pid = nix::unistd::Pid::from_raw(match &self.process {
            Some(c) => c.id() as i32,
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
    fn runtime(&self) -> Result<Duration, ()> {
        match &self.start {
            Some(s) => Ok(s.elapsed()),
            None => Err(()),
        }
    }
}
