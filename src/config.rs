use crate::checker;
use crate::executable::Language;
use crate::test::TestCase;
use clap::*;
use indicatif::{MultiProgress, ProgressDrawTarget};
use itertools::EitherOrBoth::{Both, Left, Right};
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use std::cell::LazyCell;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs};

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
                let _ = File::open(s)
                    .expect("Config does not exist in specified location!")
                    .read_to_string(&mut string);
                toml::from_str(string.as_str()).expect("Illegal config! Failed to parse TOML!")
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
        testcases: cp
            .input
            .unwrap_or(vec![])
            .iter()
            .zip(cp.output.unwrap_or(vec![]).iter())
            .zip_longest(cp.points.unwrap_or(vec![]).iter())
            .map(move |eob| match eob {
                Both((a, b), c) => TestCase {
                    input: a.to_string(),
                    expected: b.to_string(),
                    points: *c,
                },
                Left((a, b)) => {
                    info!("Found test case without any points! Falling back to zero points.");
                    TestCase {
                        input: a.to_string(),
                        expected: b.to_string(),
                        points: 0,
                    }
                }
                Right(c) => {
                    error!("Points without any I/O! Did you forget to add the cases?");
                    TestCase {
                        input: "".into(),
                        expected: "".into(),
                        points: *c,
                    }
                }
            })
            .collect(),
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
        format: match &cp.format {
            Some(s) => s.into(),
            None => "{name}_{num}_{id}_{filename}.{extension}".into(),
        },
        orderby: cp.orderby.unwrap_or(Orderby::Id),
    };
    config
}

pub fn get_config() -> Result<&'static Lazy<Config>, String> {
    Ok(&CONFIG)
}

pub fn generate_regex(format: &str) -> Regex {
    // Predefined placeholders and their regex patterns
    let placeholders = HashMap::from([
        ("name", "(?P<name>[a-zA-Z][a-zA-Z0-9_]*)"), // Starts with a letter, allows alnum + underscore
        ("alpha", "(?P<alpha>[a-zA-Z]+)"),           // Only letters
        ("num", "(?P<num>\\d+)"),                    // Only numbers
        ("alnum", "(?P<alnum>[a-zA-Z0-9]+)"),        // Letters & numbers
        ("word", "(?P<word>\\w+)"),                  // Word (letters, numbers, underscore)
        ("filename", "(?P<filename>\\w+)"),          // Word (letters, numbers, underscore)
        ("id", "(?P<id>\\d+)"),                      // Numeric ID
        ("extension", "(?P<extension>\\w+)"),        // File extension (word characters)
    ]);

    // Replace placeholders with corresponding regex patterns
    let mut pattern = format.to_string();
    for (key, value) in &placeholders {
        pattern = pattern.replace(&format!("{{{}}}", key), value);
    }

    // Escape the dot (.) for file extensions
    pattern = pattern.replace(".", "\\.");

    Regex::new(&format!("^{}$", pattern)).unwrap()
}

impl From<&str> for Language {
    fn from(value: &str) -> Language {
        match value {
            "java" => Language::Java,
            "jar" => Language::Java,
            "cpp" => Language::Cpp,
            "c" => Language::C,
            "rs" => Language::Rust,
            "py" => Language::Python,
            _ => Language::Unknown("".into()),
        }
    }
}
#[deprecated]
pub fn match_ext(s: &str) -> Language {
    Language::from(s)
}

pub static TEMPDIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let foldername = format!(
        "/tmp/apcs-tester-tmp-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos()
    );
    fs::create_dir(foldername.clone()).unwrap();
    PathBuf::from(foldername)
});

pub static CONFIG: Lazy<Config> = Lazy::new(load_config);

#[derive(Serialize, Deserialize)]
struct ConfigParams {
    entry: Option<String>,
    lang: Option<String>,
    args: Option<Vec<String>>,
    target: Option<PathBuf>,
    input: Option<Vec<String>>,
    output: Option<Vec<String>>,
    points: Option<Vec<u64>>,
    timeout: Option<u64>,
    memory: Option<u64>,
    threads: Option<u64>,
    checker: Option<String>,
    allow: Option<Vec<String>>,
    format: Option<String>,
    orderby: Option<Orderby>,
}

#[derive(Clone, Serialize)]
pub struct Config {
    pub entry: Option<String>,
    pub lang: Language,
    pub args: Vec<String>,
    pub target: PathBuf,
    pub testcases: Vec<TestCase>,
    pub timeout: u64,
    pub memory: u64,
    pub threads: u64,
    pub checker: checker::Type,
    pub allow: Vec<String>,
    pub format: String,
    pub orderby: Orderby,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Orderby {
    Name,
    Id,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            entry: None,
            lang: Language::Guess,
            args: vec![],
            target: env::current_dir().unwrap(),
            testcases: vec![],
            timeout: 10000,
            memory: 10,
            threads: 5,
            checker: checker::Type::AST,
            allow: vec![],
            format: "{name}_{num}_{id}_{filename}.{extension}".into(),
            orderby: Orderby::Id,
        }
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Language: {:?}", self.lang)?;
        writeln!(f, "Args: {:?}", self.args)?;
        writeln!(f, "Target: {:?}", self.target)?;
        writeln!(f, "Test Cases: {:?}", self.testcases)?;
        writeln!(f, "Timeout: {:?}", self.timeout)?;
        writeln!(f, "Memory: {:?}MB", self.memory)?;
        writeln!(f, "Threads: {:?}", self.threads)?;
        writeln!(f, "Checker: {:?}", self.checker)?;
        writeln!(f, "Allow: {:?}", self.allow)
    }
}

#[derive(Debug, Parser, Clone)]
pub struct Args {
    /// subcommands
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy)]
pub enum CommandType {
    Init,
    Run,
    Test,
    Format,
}

#[derive(Debug, Subcommand, Clone)]
pub enum Command {
    /// initialize the tests
    Init {
        /// do not output any logs except for panic(fatal errors)
        #[clap(short, long)]
        silent: bool,
        /// do not output any logs except for errors
        #[clap(short, long)]
        quiet: bool,
    },
    /// run the tests
    Run {
        /// Test functionality
        #[clap(short, long)]
        test: Option<String>,
        /// verbose mode
        #[clap(short, long)]
        verbose: bool,
        /// debug mode
        #[clap(long)]
        debug: bool,
        /// trace mode
        #[clap(long)]
        trace: bool,
        /// quiet mode
        #[clap(short, long)]
        quiet: bool,
        /// silent mode
        #[clap(short, long)]
        silent: bool,
        /// log level
        #[clap(short, long)]
        log_level: Option<u32>,
        /// configuration file for tests
        #[clap(long)]
        config: Option<PathBuf>,
        /// output file or directory for results
        #[clap(short, long)]
        output: Option<PathBuf>,
        /// dry-run and just execute, don't input anything.
        #[clap(long)]
        dry_run: bool,
        /// leave artifacts
        #[clap(long, short)]
        artifacts: bool,
    },
    /// test features
    Test,
    Format,
}

impl Args {
    pub fn get_config(&self) -> Option<&PathBuf> {
        match &self.command {
            Command::Run { config, .. } => config.as_ref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SimpleOpts {
    pub mode: CommandType,
    /// Test functionality
    pub test: Option<String>,
    /// verbose mode
    pub verbose: bool,
    /// debug mode
    pub debug: bool,
    /// trace mode
    pub trace: bool,
    /// quiet mode
    pub quiet: bool,
    /// silent mode
    pub silent: bool,
    /// log level
    pub log_level: Option<u32>,
    /// configuration file for tests
    pub config: PathBuf,
    /// output file or directory for results
    pub output: Option<PathBuf>,
    /// dry-run and just execute, don't input anything.
    pub dry_run: bool,
    /// leave artifacts
    pub artifacts: bool,
}
impl SimpleOpts {
    pub fn new() -> Self {
        debug!("converting ARGS into SimpleOpts: {:?}", ARGS);
        (*ARGS).clone().into()
    }
}

impl Default for SimpleOpts {
    fn default() -> Self {
        SimpleOpts {
            mode: CommandType::Init,
            test: None,
            verbose: false,
            debug: false,
            trace: false,
            quiet: false,
            silent: false,
            log_level: None,
            config: env::current_dir()
                .unwrap()
                .join(PathBuf::from_str("tests.toml").unwrap()),
            output: None,
            dry_run: true,
            artifacts: false,
        }
    }
}

impl From<Lazy<Args>> for SimpleOpts {
    fn from(value: Lazy<Args>) -> Self {
        value.clone().into()
    }
}

impl From<Args> for SimpleOpts {
    fn from(value: Args) -> Self {
        let mut ret = SimpleOpts::default();
        match value.command {
            Command::Init { silent, quiet } => {
                ret.quiet = quiet;
                ret.silent = silent;
            }
            Command::Run {
                test,
                verbose,
                debug,
                trace,
                quiet,
                silent,
                log_level,
                config,
                output,
                dry_run,
                artifacts,
            } => {
                ret.mode = CommandType::Run;
                ret.test = test;
                ret.verbose = verbose;
                ret.debug = debug;
                ret.trace = trace;
                ret.quiet = quiet;
                ret.silent = silent;
                ret.log_level = log_level;
                ret.config = match config {
                    None => {
                        debug!("Probing for test toml.");
                        let toml: Option<PathBuf> = None;
                        for i in env::current_dir().unwrap().read_dir().unwrap() {
                            let res = i.unwrap();
                            if res.path().extension().unwrap().to_str().unwrap() == "toml" {
                                if toml.is_some() {
                                    error!(
                                        "apcs-tester found two tomls! Specify which one to use!"
                                    );
                                    panic!("failed to determine what to use.");
                                }
                            }
                        }
                        match toml {
                            Some(s) => s,
                            None => {
                                error!(
                                    "Since user did not give config, Probed for config in cd: {}",
                                    env::current_dir().unwrap().to_str().unwrap()
                                );
                                error!("However, failed to find a toml file.");
                                panic!("failed to find config.");
                            }
                        }
                    }
                    Some(p) => {
                        if !(p.is_file() || p.extension().unwrap().to_str().unwrap() == "toml") {
                            error!(
                                "Unrecognized file format or illegal path: {}",
                                p.to_str().unwrap()
                            );
                        }
                        p
                    }
                };
                ret.output = output;
                ret.dry_run = dry_run;
                ret.artifacts = artifacts;
            }
            _ => {}
        }
        return ret;
    }
}

pub static ARGS: Lazy<Args> = Lazy::new(Args::parse);
pub static SIMPLEOPTS: Lazy<SimpleOpts> = Lazy::new(SimpleOpts::new);

pub fn proc_args() {
    match &ARGS.command {
        Command::Init { silent, quiet } => {
            if !*quiet && !*silent {
                info!(
                    "Initializing test in {}",
                    env::current_dir().unwrap().to_str().unwrap()
                );
            }
        }
        Command::Run {
            test,
            verbose,
            debug,
            trace,
            output,
            ..
        } => {
            if *test != None {
                info!("Test mode is enabled. Ignoring rest of arguments.");
            }
            if *verbose {
                info!("Verbose mode enabled");
            };
            if *debug {
                debug!("Debug mode enabled");
            };
            if *trace {
                trace!("Trace mode enabled");
            }

            if *output == None {
                info!("No output file or directory specified. falling back to stdout.");
            } else {
                let tmp = output.clone().unwrap();
                if tmp.is_dir() {
                    unimplemented!("Output is a directory! Not supported yet.");
                } else {
                    info!("Output file: {}", tmp.display());
                    match tmp
                        .extension()
                        .expect("Expected file format!")
                        .to_str()
                        .unwrap()
                    {
                        "json" => {
                            info!("Output format: JSON");
                        }
                        "txt" => {
                            info!("Output format: Plaintext");
                        }
                        _ => {
                            error!(
                                "Unsupported output format: {}",
                                tmp.extension()
                                    .expect("Expected file format!")
                                    .to_str()
                                    .unwrap()
                            );
                            info!("falling back to stdout.");
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

pub static MULTIPROG: Lazy<Mutex<MultiProgress>> =
    Lazy::new(|| Mutex::new(MultiProgress::with_draw_target(ProgressDrawTarget::stdout())));

pub const KNOWN_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "java", "jar", "c", "cpp", "rs", "py", "tar", "tar.gz", "gz", "zip",
    ]
    .into()
});
