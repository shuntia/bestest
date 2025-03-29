use std::{
    fmt::{Display, Formatter},
    path::PathBuf,
    process::ChildStdout,
    time,
};

use nix::sys::signal::Signal;
#[derive(Debug)]
pub struct Error {
    pub description: String,
}
impl Error {
    pub fn new(description: &'static str) -> Self {
        Self {
            description: description.into(),
        }
    }
}
impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "error: {}", self.description)
    }
}
impl std::error::Error for Error {}

pub trait Runner: Send {
    fn new(p: PathBuf) -> Result<Self, Error>
    where
        Self: Sized;
    fn running(&self) -> bool;
    fn run(&mut self) -> Result<(), Error>;
    fn get_lang(&self) -> crate::executable::Language;
    fn stdin(&mut self, s: String) -> Result<(), String>;
    fn stdout(&mut self) -> Option<&mut ChildStdout>;
    fn read_all(&mut self) -> Result<String, String>;
    fn runtime(&self) -> Result<time::Duration, ()>;
    fn signal(&mut self, s: Signal) -> Result<(), String>;
}
