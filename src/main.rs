use env_logger;
use log::LevelFilter;
#[allow(unused)]
use log::{debug, error, info, trace, warn};
use std::{collections::HashSet, fs::File, io::Write, path::PathBuf, process::exit};
use walkdir::WalkDir;
pub mod checker;
pub mod config;
pub mod executable;
pub mod lang;
pub mod test;
pub mod unpacker;
use checker::check_dir;
use config::*;

#[tokio::main]
async fn main() {
    //collect args
    let args = &config::SIMPLEOPTS;
    //log setup
    env_logger::builder()
        .filter_level(match args {
            _ if args.debug => LevelFilter::Debug,
            _ if args.verbose => LevelFilter::Info,
            _ if args.quiet => LevelFilter::Error,
            _ if args.silent => LevelFilter::Off,
            _ => LevelFilter::Info,
        })
        .init();
    match &args.mode {
        CommandType::Init => {
            info!("creating bare config file...");
            let mut f = File::create("config.toml").unwrap();
            let buf = toml::to_string_pretty(&Config::default()).unwrap();
            f.write(buf.as_bytes()).expect("failed to write to config!");
            exit(0);
        }
        _ => {}
    }
    info!("Welcome to the APCS Homework tester!");
    let config = &CONFIG;
    debug!("Config:\n{}", (*config).clone());
    debug!("Starting safety checks...");
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
    let res = test::test_dirs(exec);
    info!("{:?}", res.await);
    println!("Done!")
}
