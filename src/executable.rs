use log::{info, warn};
use serde::Serialize;
use std::path::PathBuf;
use strum_macros::EnumIter;
use walkdir::WalkDir;
use zip::ZipArchive;
use zip::result::ZipResult;
impl From<PathBuf> for Language {
    fn from(value: PathBuf) -> Self {
        match match value.extension() {
            Some(s) => s.to_str().unwrap(),
            None => {
                if value.is_dir() {
                    info!("Guessing file type from directory. This may take a while...");
                    for e in WalkDir::new(&value) {
                        match Self::from(e.unwrap().into_path()) {
                            Self::Unknown(_) => continue,
                            l @ (Language::Java
                            | Language::Cpp
                            | Language::C
                            | Language::Rust
                            | Language::Python
                            | Language::Guess) => {
                                return l;
                            }
                        }
                    }
                    warn!("Program could not find any program file within {value:?}");
                    return Self::Unknown(value.as_path().to_str().unwrap().to_owned());
                }
                return Self::Unknown(value.as_path().to_str().unwrap().to_owned());
            }
        } {
            "jar" | "java" => Self::Java,
            "cpp" => Self::Cpp,
            "c" => Self::C,
            "rs" => Self::Rust,
            "py" => Self::Python,
            "zip" | "tar" | "tar.gz" => {
                if contains_in_zip(&value, "Cargo.toml").unwrap() {
                    Self::Rust
                } else if contains_in_zip(&value, "main.cpp").unwrap() {
                    Self::Cpp
                } else if contains_in_zip(&value, "main.c").unwrap() {
                    Self::C
                } else if contains_in_zip(&value, "main.py").unwrap() {
                    Self::Python
                } else if contains_in_zip(&value, "Main.java").unwrap() {
                    Self::Java
                } else {
                    warn!("couldn't find entry point!");
                    Self::Unknown(value.as_path().to_str().unwrap().to_owned())
                }
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
#[non_exhaustive]
pub enum Language {
    Java,
    Cpp,
    C,
    Rust,
    Python,
    Unknown(String),
    Guess,
}
