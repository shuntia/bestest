use crate::checker;
use crate::executable::Language;
use clap::*;
use once_cell::sync::Lazy;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
fn load_config() -> Config {
    let cp: ConfigParams =
        match ARGS.get_config().unwrap() {
            s if s.extension().expect(
                "File extension not found! Config format guessing is not implemented yet!",
            ) == "json" =>
            {
                serde_json::from_reader(File::open(s).unwrap())
                    .expect("Illegal config! Failed to parse JSON!")
            }
            s if s.extension().expect(
                "File extension not found! Config format guessing is not implemented yet!",
            ) == "toml" =>
            {
                let mut string = String::new();
                File::open(s).unwrap().read_to_string(&mut string);
                toml::from_str(string.as_str()).expect("Illegal config! Failed to parse JSON!")
            }
            _ => {
                panic!("File extension not found! Config format guessing is not implemented yet!");
            }
        };
    if cp.target == None {
        error!("What do you mean target is none? Why are you running this program!?");
        exit(1);
    }
    let config = Config {
        entry: cp.entry,
        lang: Language::Guess,
        target: cp.target.unwrap_or(std::env::current_dir().unwrap()),
        args: cp.args.unwrap_or(vec![]),
        input: cp.input.unwrap_or(vec![]),
        output: cp.output.unwrap_or(vec![]),
        points: cp.points.unwrap_or(vec![]),
        timeout: cp.timeout.unwrap_or(5),
        memory: cp.memory.unwrap_or(1024),
        threads: cp.threads.unwrap_or(4),
        checker: cp
            .checker
            .map(|x| match x.as_str() {
                "static" => checker::Type::Static,
                "ast" => checker::Type::AST,
                _ => checker::Type::AST,
            })
            .unwrap(),
        allow: cp.allow.unwrap_or(vec![]),
    };
    if config.input.len() != config.output.len() {
        warn!("CONFIG: potential misalignment in input-output pair.");
    };
    config
}

pub fn get_config() -> Result<&'static Lazy<Config>, String> {
    Ok(&CONFIG)
}

pub fn from(value: String) -> Language {
    match value.as_str() {
        "java" => Language::Java,
        "jar" => Language::Java,
        "cpp" => Language::Cpp,
        "c" => Language::C,
        "rs" => Language::Rust,
        "py" => Language::Python,
        _ => Language::Unknown("".into()),
    }
}

#[deprecated]
pub fn match_ext(s: &str) -> Language {
    from(s.to_owned())
}

pub static CONFIG: Lazy<Config> = Lazy::new(load_config);

#[derive(Serialize, Deserialize)]
struct ConfigParams {
    entry: Option<String>,
    lang: Option<String>,
    args: Option<Vec<String>>,
    target: Option<PathBuf>,
    input: Option<Vec<Vec<String>>>,
    output: Option<Vec<String>>,
    points: Option<Vec<u64>>,
    timeout: Option<u64>,
    memory: Option<u64>,
    threads: Option<u64>,
    checker: Option<String>,
    allow: Option<Vec<String>>,
}

#[derive(Clone, Serialize)]
pub struct Config {
    pub entry: Option<String>,
    pub lang: Language,
    pub args: Vec<String>,
    pub target: PathBuf,
    pub input: Vec<Vec<String>>,
    pub output: Vec<String>,
    pub points: Vec<u64>,
    pub timeout: u64,
    pub memory: u64,
    pub threads: u64,
    pub checker: checker::Type,
    pub allow: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            entry: None,
            lang: Language::Guess,
            args: vec![],
            target: PathBuf::new(),
            input: vec![],
            output: vec![],
            points: vec![],
            timeout: 500,
            memory: 10,
            threads: 5,
            checker: checker::Type::AST,
            allow: vec![],
        }
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Language: {:?}", self.lang)?;
        writeln!(f, "Args: {:?}", self.args)?;
        writeln!(f, "Target: {:?}", self.target)?;
        writeln!(f, "Input: {:?}", self.input)?;
        writeln!(f, "Output: {:?}", self.output)?;
        writeln!(f, "Points: {:?}", self.points)?;
        writeln!(f, "Timeout: {:?}", self.timeout)?;
        writeln!(f, "Memory: {:?}MB", self.memory)?;
        writeln!(f, "Threads: {:?}", self.threads)?;
        writeln!(f, "Checker: {:?}", self.checker)?;
        writeln!(f, "Allow: {:?}", self.allow)
    }
}

#[derive(Parser)]
pub struct Args {
    /// subcommands
    #[clap(subcommand)]
    pub command: Command,
    /// Test functionality
    #[clap(short, long)]
    pub test: Option<String>,
    /// Verbose mode
    #[clap(short, long)]
    pub verbose: bool,
    /// Debug mode
    #[clap(long)]
    pub debug: bool,
    /// Quiet mode
    #[clap(short, long)]
    pub quiet: bool,
    /// Silent mode
    #[clap(short, long)]
    pub silent: bool,
    /// Log level
    #[clap(short, long)]
    pub log_level: Option<u32>,
    /// Configuration file for tests
    #[clap(long)]
    pub config: Option<PathBuf>,
    /// Input file or directory
    #[clap(short, long)]
    pub input: Option<PathBuf>,
    /// Output file or directory for results
    #[clap(short, long)]
    pub output: Option<PathBuf>,
    /// dry-run and just execute, don't input anything.
    #[clap(long)]
    pub dry_run: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// initialize the tests
    Init {},
    /// run the tests
    Run,
    Format,
}

impl Args {
    pub fn get_config(&self) -> Option<&PathBuf> {
        match &self.config {
            Some(s) => Some(s),
            None => self.input.as_ref(),
        }
    }
}

pub static ARGS: Lazy<Args> = Lazy::new(Args::parse);
