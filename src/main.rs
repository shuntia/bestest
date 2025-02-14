use std::path::PathBuf;
use std::*;

use clap::*;
use env_logger;
use env_logger::Env;
use log::{debug, error, info, trace, warn};
mod config;
#[derive(Parser)]
/// APCS tester - a tool for testing APCS programs submitted by students.
struct Args {
    /// Verbose mode
    #[clap(short, long)]
    verbose: bool,
    /// Debug mode
    #[clap(long)]
    debug: bool,
    /// Quiet mode
    #[clap(short, long)]
    quiet: bool,
    /// Silent mode
    #[clap(short, long)]
    silent: bool,
    /// Log level
    #[clap(short, long)]
    log_level: Option<u32>,
    /// Configuration file for tests
    #[clap(long)]
    config: Option<PathBuf>,
    /// Input file or directory
    #[clap(short, long)]
    input: Option<PathBuf>,
    /// Output file or directory for results
    #[clap(short, long)]
    output: Option<PathBuf>,
}

fn main() {
    env_logger::init();
    //argument checking
    let args = Args::parse();
    {
        if args.verbose {
            log::set_max_level(log::LevelFilter::Debug);
            info!("Verbose mode enabled");
        }
        if args.debug {
            log::set_max_level(log::LevelFilter::Trace);
            debug!("Debug mode enabled");
        }
        if args.config == None {
            error!("No configuration file specified");
        }
        if args.input == None {
            error!("No input file or directory specified");
        }
        if args.output == None {
            info!("No output file or directory specified. falling back to stdout.");
        } else {
            let tmp = &args.output.unwrap();
            if tmp.is_dir() {
                panic!("Output is a directory! Not supported yet.");
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
                    _ => {
                        panic!(
                            "Unsupported output format: {}",
                            tmp.extension()
                                .expect("Expected file format!")
                                .to_str()
                                .unwrap()
                        );
                    }
                }
            }
        }
    }
    //parse configuration file
    let config = config::load_config(args.config.unwrap()).unwrap();
    {
        info!("Configuration loaded");
        info!("Language: {}", config.lang);
        info!("Executor: {}", config.exec);
        info!("Arguments: {:?}", config.args);
        info!("Input: {}", config.input);
        info!("Output: {}", config.output);
        info!("Timeout: {}s", config.timeout);
        info!("Memory: {}MB", config.memory);
        info!("Threads: {}", config.threads);
    }
}
