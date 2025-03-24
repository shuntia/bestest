use crate::checker::check_dir;
use crate::config::*;
use env_logger;
#[allow(unused)]
use log::{debug, error, info, trace, warn};
use std::{any::Any, collections::HashSet, fs::File, io::Write, path::PathBuf, process::exit};
use walkdir::WalkDir;
pub mod checker;
pub mod config;
pub mod executable;
pub mod lang;
pub mod test;

#[tokio::main]
async fn main() {
    //argument checking
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();
    let args = &config::ARGS;
    match &args.command {
        Command::Init {} => {
            info!("creating bare config file...");
            let mut f = File::create("config.toml").unwrap();
            let buf = toml::to_string_pretty(&Config::default()).unwrap();
            f.write(buf.as_bytes()).expect("failed to write to config!");
            exit(0);
        }
        _ => {}
    }
    argmsg(args);
    let config = &CONFIG;
    info!("Config:\n{}", (*config).clone());
    info!("Starting safety checks...");
    let check_result = check_dir(config.target.clone().into()).await.unwrap();
    if check_result.is_empty() {
        info!("All checks passed.");
    } else {
        warn!("Dangerous code detected.");
        for i in &check_result {
            warn!("{:?}", i);
        }
        warn!("Aborting check for those files.");
        info!("NOTE: if you want to allow potentially dangerous operations, configure it in config.json.");
    }
    // get the executables and remove dangerous files.
    let mut exec: HashSet<PathBuf> = HashSet::new();
    let walk = WalkDir::new(config::get_config().unwrap().target.clone());
    for i in walk {
        exec.insert(i.unwrap().into_path());
    }
    for i in check_result {
        exec.remove(&i.0);
    }
}

fn argmsg(args: &Args) {
    if args.test != None {
        println!("Test mode is enabled. Ignoring rest of arguments.");
        return;
    }
    if args.verbose {
        log::set_max_level(log::LevelFilter::Debug);
        info!("Verbose mode enabled");
    }
    if args.debug {
        log::set_max_level(log::LevelFilter::Trace);
        debug!("Debug mode enabled");
    }
    if args.config == None {
        error!("No configuration file specified! The program will attempt to find one inside the target directory.");
    }
    if args.input == None {
        error!("No input file or directory specified");
        if args.config == None {
            panic!("No input directory nor config file! Tester does not know what to do!");
        }
    } else if args.input.clone().unwrap().is_file() {
        if args.config == None {
            panic!("Cannot probe config file with only one provided file.");
        }
    }
    if args.output == None {
        info!("No output file or directory specified. falling back to stdout.");
    } else {
        let tmp = args.output.clone().unwrap();
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
