use crate::checker::{self, Type};
use crate::executable::Language;
use crate::test::TestCase;
use anyhow::{Ok, Result};
use clap::{Parser, Subcommand};
use core::fmt::{Display, Formatter};
use core::str::FromStr as _;
use indicatif::{MultiProgress, ProgressDrawTarget};
use itertools::EitherOrBoth::{Both, Left, Right};
use itertools::Itertools as _;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::env;
#[cfg(not(feature = "gui"))]
use std::fs::File;
use std::fs::create_dir_all;
#[cfg(not(feature = "gui"))]
use std::io::Read as _;
use std::path::PathBuf;
use std::process::exit;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

#[expect(unused_imports)]
use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
fn load_config() -> Config {
    #[cfg(not(feature = "gui"))]
    let cp: ConfigParams = match ARGS.get_config() {
        Some(config) => match config {
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
        },

        None => ConfigParams::default(),
    };
    #[cfg(feature = "gui")]
    let cp = crate::gui::app::get_config();
    if cp.entry.is_none() {
        error!("User did not specify entry point! Falling back to\"Main\".");
    }
    if cp.target.is_none() {
        error!("Could not find target!");
        exit(1);
    }

    let config = Config {
        entry: cp.entry.unwrap_or_else(|| "Main".into()),
        lang: Language::Guess,
        target: cp
            .target
            .unwrap_or_else(|| std::env::current_dir().unwrap()),
        args: cp.args.unwrap_or_default(),
        testcases: cp
            .input
            .unwrap_or_default()
            .iter()
            .zip(cp.output.unwrap_or_default().iter())
            .zip_longest(cp.points.unwrap_or_default().iter())
            .map(move |eob| match eob {
                Both((a, b), c) => {
                    return TestCase {
                        input: a.to_string(),
                        expected: b.to_string(),
                        points: *c,
                    };
                }
                Left((a, b)) => {
                    debug!("Found test case without any points! Falling back to zero points.");
                    return TestCase {
                        input: a.to_string(),
                        expected: b.to_string(),
                        points: 0,
                    };
                }
                Right(c) => {
                    error!("Points without any I/O! Did you forget to add the cases?");
                    TestCase {
                        input: String::new(),
                        expected: String::new(),
                        points: *c,
                    }
                }
            })
            .collect(),
        timeout: cp.timeout.unwrap_or(5),
        memory: cp.memory.unwrap_or(1024),
        #[allow(clippy::unwrap_used)]
        threads: cp.threads.unwrap_or(num_cpus::get().try_into().unwrap()),
        checker: cp.checker.unwrap_or(Type::Static),
        allow: cp.allow.unwrap_or_default(),
        format: cp.format.as_ref().map_or_else(
            || "{name}_{num}_{id}_{filename}.{extension}".into(),
            |s| s.into(),
        ),
        orderby: cp.orderby.unwrap_or(Orderby::Id),
        dependencies: cp.dependencies.unwrap_or_default(),
    };
    return config;
}

pub fn get_config() -> Result<&'static LazyLock<Config>> {
    return Ok(&CONFIG);
}

#[must_use]
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
    let mut pattern = format.to_owned();
    for (key, value) in &placeholders {
        pattern = pattern.replace(&format!("{{{key}}}"), value);
    }

    // Escape the dot (.) for file extensions
    pattern = pattern.replace('.', "\\.");

    Regex::new(&format!("^{pattern}$")).unwrap()
}

impl From<&str> for Language {
    fn from(value: &str) -> Self {
        match value {
            "java" => Self::Java,
            "jar" => Self::Java,
            "cpp" => Self::Cpp,
            "c" => Self::C,
            "rs" => Self::Rust,
            "py" => Self::Python,
            _ => Self::Unknown(String::new()),
        }
    }
}
#[deprecated]
#[must_use]
pub fn match_ext(s: &str) -> Language {
    return Language::from(s);
}

pub static TEMPDIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let foldername = format!(
        "/tmp/apcs-tester-tmp-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos()
    );
    create_dir_all(foldername.clone()).unwrap();
    return PathBuf::from(foldername);
});

pub static CONFIG: std::sync::LazyLock<Config> = std::sync::LazyLock::new(load_config);

#[derive(Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConfigParams {
    pub entry: Option<String>,
    pub lang: Option<String>,
    pub args: Option<Vec<String>>,
    pub target: Option<PathBuf>,
    pub input: Option<Vec<String>>,
    pub output: Option<Vec<String>>,
    pub points: Option<Vec<u64>>,
    pub timeout: Option<u64>,
    pub memory: Option<u64>,
    pub threads: Option<u64>,
    pub checker: Option<Type>,
    pub allow: Option<Vec<String>>,
    pub format: Option<String>,
    pub orderby: Option<Orderby>,
    pub dependencies: Option<Vec<PathBuf>>,
}

impl Default for ConfigParams {
    fn default() -> Self {
        Self {
            entry: None,
            lang: Some("Guess".into()),
            args: Some(vec![]),
            target: Some(env::current_dir().unwrap()),
            input: Some(vec![]),
            output: Some(vec![]),
            points: Some(vec![]),
            timeout: Some(10000),
            memory: None,
            threads: Some(5),
            checker: Some(Type::Static),
            format: Some("{name}_{num}_{id}_{filename}.{extension}".into()),
            allow: Some(vec![]),
            orderby: Some(Orderby::Name),
            dependencies: Some(vec![]),
        }
    }
}

#[derive(Clone, Serialize)]
#[non_exhaustive]
pub struct Config {
    pub entry: String,
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
    pub dependencies: Vec<PathBuf>,
}

#[derive(Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Orderby {
    Name,
    Id,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            entry: String::new(),
            lang: Language::Guess,
            args: vec![],
            target: env::current_dir().unwrap(),
            testcases: vec![],
            timeout: 10000,
            memory: 10,
            threads: 5,
            checker: checker::Type::Static,
            allow: vec![],
            format: "{name}_{num}_{id}_{filename}.{extension}".into(),
            orderby: Orderby::Id,
            dependencies: vec![],
        }
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "Language: {:?}", self.lang)?;
        writeln!(f, "Args: {:?}", self.args)?;
        writeln!(f, "Target: {:?}", self.target)?;
        writeln!(f, "Test Cases: {:?}", self.testcases)?;
        writeln!(f, "Timeout: {:?}", self.timeout)?;
        writeln!(f, "Memory: {:?}MB", self.memory)?;
        writeln!(f, "Threads: {:?}", self.threads)?;
        writeln!(f, "Checker: {:?}", self.checker)?;
        return writeln!(f, "Allow: {:?}", self.allow);
    }
}

#[derive(Debug, Parser, Clone)]
#[non_exhaustive]
pub struct Args {
    /// subcommands
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum CommandType {
    Init,
    Run,
    Test,
    Format,
}

#[derive(Debug, Subcommand, Clone)]
#[non_exhaustive]
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
    pub const fn get_config(&self) -> Option<&PathBuf> {
        match &self.command {
            Command::Run { config, .. } => return config.as_ref(),
            Command::Init { .. } | Command::Test | Command::Format => return None,
        }
    }
}

impl Default for Args {
    fn default() -> Self {
        Self {
            command: Command::Run {
                test: None,
                verbose: false,
                debug: false,
                trace: false,
                quiet: false,
                silent: false,
                log_level: None,
                config: None,
                output: Some(PathBuf::from_str("/tmp/output.toml").unwrap()),
                dry_run: false,
                artifacts: false,
            },
        }
    }
}

#[derive(Debug, Clone)]
#[non_exhaustive]
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
    #[must_use]
    pub fn new() -> Self {
        debug!("converting ARGS into SimpleOpts: {ARGS:?}");
        (*ARGS).clone().into()
    }
}

impl Default for SimpleOpts {
    fn default() -> Self {
        Self {
            mode: CommandType::Run,
            test: None,
            verbose: false,
            debug: false,
            trace: false,
            quiet: false,
            silent: false,
            log_level: None,
            config: env::current_dir()
                .unwrap()
                .join(PathBuf::from_str("config.toml").unwrap()),
            output: None,
            dry_run: true,
            artifacts: false,
        }
    }
}

impl From<Lazy<Args>> for SimpleOpts {
    fn from(value: Lazy<Args>) -> Self {
        return value.clone().into();
    }
}

impl From<Args> for SimpleOpts {
    fn from(value: Args) -> Self {
        let mut ret = Self::default();
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
                ret.config =
                    match config {
                        None => {
                            debug!("Probing for test toml.");
                            let toml: Option<PathBuf> = None;
                            for i in env::current_dir().unwrap().read_dir().unwrap() {
                                let res = i.unwrap();
                                if let Some(s) = res.path().extension() {
                                    if s == "toml" && toml.is_some() {
                                        error!(
                                            "apcs-tester found two tomls! Specify which one to use!"
                                        );
                                        panic!("failed to determine what to use.");
                                    }
                                }
                            }
                            toml.map_or_else(|| {
                                error!(
                                    "Since user did not give config, Probed for config in cd: {}",
                                    env::current_dir().unwrap().to_str().unwrap()
                                );
                                error!("However, failed to find a toml file.");
                                panic!("failed to find config.");
                            }, |s| s)
                        }
                        Some(p) => {
                            if !(p.is_file() || p.extension().unwrap().to_str().unwrap() == "toml")
                            {
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
            Command::Test | Command::Format => {}
        }
        ret
    }
}
#[cfg(not(feature = "gui"))]
pub static ARGS: std::sync::LazyLock<Args> = std::sync::LazyLock::new(Args::parse);
#[cfg(feature = "gui")]
pub static ARGS: Lazy<Args> = Lazy::new(Args::default);
#[cfg(not(feature = "gui"))]
pub static SIMPLEOPTS: std::sync::LazyLock<SimpleOpts> = std::sync::LazyLock::new(SimpleOpts::new);
#[cfg(feature = "gui")]
pub static SIMPLEOPTS: Lazy<SimpleOpts> = Lazy::new(SimpleOpts::default);

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
            if test.is_some() {
                debug!("Test mode is enabled. Ignoring rest of arguments.");
            }
            if *verbose {
                debug!("Verbose mode enabled");
            }
            if *debug {
                debug!("Debug mode enabled");
            }
            if *trace {
                trace!("Trace mode enabled");
            }

            if output.is_none() {
                debug!("No output file or directory specified. falling back to stdout.");
            } else {
                let tmp = output.clone().unwrap();
                if tmp.is_dir() {
                    unimplemented!("Output is a directory! Not supported yet.");
                } else {
                    debug!("Output file: {}", tmp.display());
                    match tmp
                        .extension()
                        .expect("Expected file format!")
                        .to_str()
                        .unwrap()
                    {
                        "json" => {
                            debug!("Output format: JSON");
                        }
                        "txt" => {
                            debug!("Output format: Plaintext");
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
        Command::Test | Command::Format => {}
    }
}

pub static MULTIPROG: std::sync::LazyLock<Mutex<MultiProgress>> = std::sync::LazyLock::new(|| {
    return Mutex::new(MultiProgress::with_draw_target(ProgressDrawTarget::stdout()));
});

pub const KNOWN_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    return [
        "java", "jar", "c", "cpp", "rs", "py", "tar", "tar.gz", "gz", "zip",
    ]
    .into();
});

// Spinner only properly displays if you have nerd fonts installed.
pub const SPINNER: [&str; 6] = [
    "\u{ee06}", "\u{ee07}", "\u{ee08}", "\u{ee08}", "\u{ee0a}", "\u{ee0b}",
];
