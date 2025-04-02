use crate::lang::runner;
use crate::lang::runner::Runner;
use log::*;
use serde::Serialize;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use strum_macros::EnumIter;
use walkdir::WalkDir;
use zip::result::ZipResult;
use zip::ZipArchive;
#[allow(unused)]
pub struct Executable {
    path: PathBuf,
    venv: bool,
    language: Language,
}

impl Executable {
    pub fn new(path: PathBuf, venv: bool) -> Self {
        let mut acc = "".to_owned();
        let filen = path.file_name().unwrap().to_str();
        let lang = crate::config::match_ext(
            path.extension()
                .unwrap_or_else(|| {
                    let loc = filen.iter().find(|e| e == &&"").unwrap();
                    for i in (*loc).chars() {
                        acc.push(i);
                    }
                    &OsStr::new(&acc)
                })
                .to_str()
                .unwrap(),
        );
        Executable {
            path: path.clone(),
            venv,
            language: lang,
        }
    }
    pub async fn dry_run(&mut self) -> Result<Box<dyn Runner>, runner::Error> {
        self.execute("")
    }

    pub async fn run(&mut self, args: &str) -> Result<Box<dyn Runner>, runner::Error> {
        self.execute(args)
    }

    pub fn execute(&mut self, args: &str) -> Result<Box<dyn Runner>, runner::Error> {
        match &self.language {
            Language::Java => {
                return Ok(Box::new(tokio::runtime::Runtime::new().unwrap().block_on(
                    crate::lang::java::JavaRunner::new_from_venv(self.path.clone(), PathBuf::new()),
                )?))
            }
            Language::C => {}
            Language::Cpp => {}
            Language::Python => {}
            Language::Rust => {}
            Language::Unknown(_s) => {
                warn!("Unknown Language specified! Attempting to literally run the program.");
            }
            Language::Guess => {
                if self.path.extension().is_none() {
                    if self.path.is_dir() {
                        error!("Running directories is not supported yet. In future releases, the implementation of entry probing is expected.");
                        return Err(runner::Error::new(
                            "Received Directory instead of file!".into(),
                        ));
                    }
                    #[cfg(target_os = "windows")]
                    return Err(("No file extension found."));
                    if self.path.is_file() {
                        if self.path.metadata().unwrap().permissions().mode() & 0o111 != 0 {
                            warn!("Received an executable file! Running it as is.");
                            todo!();
                        }
                    }
                }
                self.language =
                    crate::config::match_ext(self.path.extension().unwrap().to_str().unwrap());
                return self.execute(args);
            }
        };
        return Err(runner::Error::new("???".into()));
    }
}
impl From<PathBuf> for Language {
    fn from(value: PathBuf) -> Self {
        match match value.extension() {
            Some(s) => s.to_str().unwrap(),
            None => {
                if value.is_dir() {
                    info!("Guessing file type from directory. This may take a while...");
                    for e in WalkDir::new(&value).into_iter() {
                        match Self::from(e.unwrap().into_path()) {
                            Language::Unknown(_) => continue,
                            l => {
                                return l;
                            }
                        }
                    }
                    warn!("Program could not find any program file within {:?}", value);
                    return Language::Unknown(value.as_path().to_str().unwrap().to_owned());
                } else {
                    return Language::Unknown(value.as_path().to_str().unwrap().to_owned());
                }
            }
        } {
            "java" => Self::Java,
            "jar" => Self::Java,
            "cpp" => Self::Cpp,
            "c" => Self::C,
            "rs" => Self::Rust,
            "py" => Self::Python,
            "zip" | "tar" | "tar.gz" => {
                if contains_in_zip(&value, "Cargo.toml").unwrap() {
                    return Self::Rust;
                }
                if contains_in_zip(&value, "main.cpp").unwrap() {
                    return Self::Cpp;
                }
                if contains_in_zip(&value, "main.c").unwrap() {
                    return Self::C;
                }
                if contains_in_zip(&value, "main.py").unwrap() {
                    return Self::Python;
                }
                if contains_in_zip(&value, "Main.java").unwrap() {
                    return Self::Java;
                }
                warn!("couldn't find entry point!");
                Self::Unknown(value.as_path().to_str().unwrap().to_owned())
            }
            _ => Self::Unknown(value.as_path().to_str().unwrap().to_owned()),
        }
    }
}

fn contains_in_zip(p: &PathBuf, target: &str) -> ZipResult<bool> {
    let file = std::fs::File::open(p)?;
    let mut archive = ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        if entry.name().ends_with(target) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, EnumIter, Serialize)]
pub enum Language {
    Java,
    Cpp,
    C,
    Rust,
    Python,
    Unknown(String),
    Guess,
}
