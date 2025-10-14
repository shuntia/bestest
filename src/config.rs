use crate::checker::{self, Type};
use crate::executable::Language;
use crate::test::TestCase;
use anyhow::Result;
use clap::{Parser, Subcommand};
use core::fmt::{Display, Formatter};
use indicatif::{MultiProgress, ProgressDrawTarget};
use itertools::EitherOrBoth::{Both, Left, Right};
use itertools::Itertools as _;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::env::{self, temp_dir};
#[cfg(not(feature = "gui"))]
use std::fs::File;
use std::fs::create_dir_all;
#[cfg(not(feature = "gui"))]
use std::io::Read as _;
use std::path::PathBuf;
use std::process::exit;
use std::sync::LazyLock;
use std::thread::available_parallelism;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use log::{debug, error, info, trace, warn};
use serde::{Deserialize, Serialize};
fn load_config() -> Config {
    #[cfg(not(feature = "gui"))]
    let cp: ConfigParams = match ARGS.get_config() {
        Some(config_path) => {
            let ext = config_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            match ext.as_str() {
                "json" => match File::open(config_path) {
                    Ok(file) => match serde_json::from_reader(file) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            error!("Failed to parse JSON config {config_path:?}: {e}");
                            ConfigParams::default()
                        }
                    },
                    Err(e) => {
                        error!("Failed to open config file {config_path:?}: {e}");
                        ConfigParams::default()
                    }
                },
                "toml" => {
                    let mut contents = String::new();
                    match File::open(config_path) {
                        Ok(mut file) => {
                            if let Err(e) = file.read_to_string(&mut contents) {
                                error!("Failed to read config file {config_path:?}: {e}");
                                ConfigParams::default()
                            } else {
                                match toml::from_str(contents.as_str()) {
                                    Ok(cfg) => cfg,
                                    Err(e) => {
                                        error!("Failed to parse TOML config {config_path:?}: {e}");
                                        ConfigParams::default()
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to open config file {config_path:?}: {e}");
                            ConfigParams::default()
                        }
                    }
                }
                _ => {
                    error!(
                        "Unsupported config extension for {config_path:?}. Falling back to defaults."
                    );
                    ConfigParams::default()
                }
            }
        }

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

    Config {
        entry: cp.entry.unwrap_or_else(|| "Main".into()),
        lang: Language::Guess,
        target: cp.target.unwrap_or_else(|| match std::env::current_dir() {
            Ok(dir) => dir,
            Err(e) => {
                warn!("Failed to obtain current directory: {e}");
                PathBuf::from(".")
            }
        }),
        args: cp.args.unwrap_or_default(),
        testcases: cp
            .input
            .unwrap_or_default()
            .iter()
            .zip(cp.output.unwrap_or_default().iter())
            .zip_longest(cp.points.unwrap_or_default().iter())
            .map(move |eob| match eob {
                Both((a, b), c) => TestCase {
                    input: a.to_string(),
                    expected: b.to_string(),
                    points: *c,
                },
                Left((a, b)) => {
                    debug!("Found test case without any points! Falling back to zero points.");
                    TestCase {
                        input: a.to_string(),
                        expected: b.to_string(),
                        points: 0,
                    }
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
        threads: cp
            .threads
            .unwrap_or_else(|| {
                available_parallelism()
                    .map(|nz| nz.get() as u64)
                    .unwrap_or(4)
            })
            .max(1),
        checker: cp.checker.unwrap_or(Type::Static),
        allow: cp.allow.unwrap_or_default(),
        format: cp.format.as_ref().map_or_else(
            || "{name}_{num}_{id}_{filename}.{extension}".into(),
            |s| s.into(),
        ),
        orderby: cp.orderby.unwrap_or(Orderby::Id),
        dependencies: cp.dependencies.unwrap_or_default(),
    }
}

#[inline]
pub fn get_config() -> Result<&'static LazyLock<Config>> {
    Ok(&CONFIG)
}

pub fn generate_regex(format: &str) -> Result<Regex, regex::Error> {
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

    Regex::new(&format!("^{pattern}$"))
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
#[inline]
pub fn match_ext(s: &str) -> Language {
    Language::from(s)
}

pub static TEMPDIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let foldername = format!(
        "{}/bestest-tmp-{}",
        temp_dir().to_string_lossy(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos()
    );
    match create_dir_all(&foldername) {
        Ok(()) => PathBuf::from(&foldername),
        Err(e) => {
            warn!(
                "Failed to create temporary directory {foldername}: {e}. Falling back to system temp dir"
            );
            std::env::temp_dir()
        }
    }
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
            target: Some(env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
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

#[derive(Clone, Serialize, Deserialize, Debug)]
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
            target: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
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
        writeln!(f, "Allow: {:?}", self.allow)
    }
}

#[derive(Debug, Parser, Clone)]
#[non_exhaustive]
/// Bestest is the Bestest, efficient, speedy tester.
///
/// Refer to https://github.com/shuntia/bestest for how to configure config.toml.
///
/// This tester heavily utilizes the tokio runtime to efficiently await tasks.
pub struct Args {
    /// verbose mode
    #[clap(short, long, global = true)]
    pub verbose: bool,
    /// debug mode
    #[clap(long, global = true)]
    pub debug: bool,
    /// trace mode
    #[clap(long, global = true)]
    pub trace: bool,
    /// quiet mode
    #[clap(short, long, global = true)]
    pub quiet: bool,
    /// silent mode
    #[clap(short, long, global = true)]
    pub silent: bool,
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
    Init,
    /// run the tests
    Run {
        /// Test functionality
        #[clap(short, long)]
        test: Option<String>,
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
        /// sort results before printing
        #[clap(long)]
        sort: bool,
    },
    /// test features
    Test,
    Format,
}

impl Args {
    pub const fn get_config(&self) -> Option<&PathBuf> {
        match &self.command {
            Command::Run { config, .. } => config.as_ref(),
            Command::Init | Command::Test | Command::Format => None,
        }
    }
}

impl Default for Args {
    fn default() -> Self {
        Self {
            verbose: false,
            debug: false,
            trace: false,
            quiet: false,
            silent: false,
            command: Command::Run {
                test: None,
                log_level: None,
                config: None,
                output: Some(PathBuf::from("config.toml")),
                dry_run: false,
                artifacts: false,
                sort: false,
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
    /// sort results before printing
    pub sort: bool,
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
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(PathBuf::from("config.toml")),
            output: None,
            dry_run: true,
            artifacts: false,
            sort: false,
        }
    }
}

impl From<Lazy<Args>> for SimpleOpts {
    #[inline]
    fn from(value: Lazy<Args>) -> Self {
        value.clone().into()
    }
}

impl From<Args> for SimpleOpts {
    fn from(value: Args) -> Self {
        let mut ret = Self {
            verbose: value.verbose,
            debug: value.debug,
            trace: value.trace,
            quiet: value.quiet,
            silent: value.silent,
            ..Self::default()
        };
        match value.command {
            Command::Init => {
                ret.mode = CommandType::Init;
            }
            Command::Run {
                test,
                log_level,
                config,
                output,
                dry_run,
                artifacts,
                sort,
            } => {
                ret.mode = CommandType::Run;
                ret.test = test;
                ret.log_level = log_level;
                ret.config = match config {
                    None => {
                        debug!("Probing for test toml.");
                        let mut found: Option<PathBuf> = None;
                        match env::current_dir() {
                            Ok(current_dir) => match current_dir.read_dir() {
                                Ok(entries) => {
                                    for entry in entries {
                                        match entry {
                                            Ok(dir_entry) => {
                                                let path = dir_entry.path();
                                                if path.extension().and_then(|ext| ext.to_str())
                                                    == Some("toml")
                                                {
                                                    if found.is_some() {
                                                        error!(
                                                            "Multiple TOML files found. Please specify which to use."
                                                        );
                                                        break;
                                                    }
                                                    found = Some(path);
                                                }
                                            }
                                            Err(e) => warn!(
                                                "Failed to inspect directory entry while probing config: {e}"
                                            ),
                                        }
                                    }
                                }
                                Err(e) => warn!(
                                    "Failed to read current directory while probing config: {e}"
                                ),
                            },
                            Err(e) => warn!(
                                "Failed to determine current directory while probing config: {e}"
                            ),
                        }
                        found.unwrap_or_else(|| {
                            warn!(
                                "Did not detect a config file; continuing with default config.toml"
                            );
                            PathBuf::from("config.toml")
                        })
                    }
                    Some(p) => {
                        let is_toml = p
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"));
                        if !p.is_file() || !is_toml {
                            error!("Unrecognized file format or illegal path: {:?}", p);
                        }
                        p
                    }
                };
                ret.output = output;
                ret.dry_run = dry_run;
                ret.artifacts = artifacts;
                ret.sort = sort;
            }
            Command::Test => {
                ret.mode = CommandType::Test;
            }
            Command::Format => {
                ret.mode = CommandType::Format;
            }
        }
        ret
    }
}
#[cfg(not(feature = "gui"))]
pub static ARGS: std::sync::LazyLock<Args> = std::sync::LazyLock::new(Args::parse);
#[cfg(feature = "gui")]
pub static ARGS: Lazy<Args> = Lazy::new(Args::default);
#[cfg(not(feature = "gui"))]
pub static SIMPLEOPTS: std::sync::LazyLock<SimpleOpts> =
    std::sync::LazyLock::new(|| ARGS.clone().into());
#[cfg(feature = "gui")]
pub static SIMPLEOPTS: Lazy<SimpleOpts> = Lazy::new(SimpleOpts::default);

pub fn proc_args() {
    let args = &*ARGS;
    match &args.command {
        Command::Init => {
            if !args.quiet && !args.silent {
                let cwd = env::current_dir()
                    .ok()
                    .and_then(|p| p.to_str().map(str::to_owned))
                    .unwrap_or_else(|| "<unknown>".into());
                info!("Initializing test in {cwd}");
            }
        }
        Command::Run { test, output, .. } => {
            if test.is_some() {
                debug!("Test mode is enabled. Ignoring rest of arguments.");
            }
            if args.verbose {
                debug!("Verbose mode enabled");
            }
            if args.debug {
                debug!("Debug mode enabled");
            }
            if args.trace {
                trace!("Trace mode enabled");
            }

            if let Some(tmp) = output.clone() {
                if tmp.is_dir() {
                    unimplemented!("Output is a directory! Not supported yet.");
                } else {
                    debug!("Output file: {}", tmp.display());
                    match tmp.extension().and_then(|ext| ext.to_str()) {
                        Some("json") => debug!("Output format: JSON"),
                        Some("txt") => debug!("Output format: Plaintext"),
                        Some(ext) => {
                            error!("Unsupported output format: {ext}");
                            info!("falling back to stdout.");
                        }
                        None => {
                            error!("Unsupported output format: <none>");
                            info!("falling back to stdout.");
                        }
                    }
                }
            } else {
                debug!("No output file or directory specified. falling back to stdout.");
            }
        }
        Command::Test | Command::Format => {}
    }
}

pub static MULTIPROG: std::sync::LazyLock<Mutex<MultiProgress>> = std::sync::LazyLock::new(|| {
    Mutex::new(MultiProgress::with_draw_target(ProgressDrawTarget::stdout()))
});

pub static KNOWN_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "java", "jar", "c", "cpp", "rs", "py", "tar", "tar.gz", "gz", "zip",
    ]
    .into()
});

#[cfg(feature = "nerdfont")]
pub const SPINNER: [&str; 6] = [
    "\u{ee06}", "\u{ee07}", "\u{ee08}", "\u{ee08}", "\u{ee0a}", "\u{ee0b}",
];

#[cfg(not(feature = "nerdfont"))]
pub const SPINNER: [&str; 6] = ["-", "\\", "|", "/", "-", "\\"];
