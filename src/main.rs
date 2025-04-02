use env_logger;
use indicatif_log_bridge::LogWrapper;
use log::LevelFilter;
#[allow(unused)]
use log::{debug, error, info, trace, warn};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Write,
    path::PathBuf,
    process::exit,
};
use tokio::fs::remove_dir_all;
pub mod checker;
pub mod config;
pub mod executable;
pub mod lang;
pub mod test;
pub mod unpacker;
use checker::{check_dirs, IllegalExpr};
use config::*;

#[tokio::main]
async fn main() {
    let args = &config::SIMPLEOPTS;
    let logger = env_logger::builder()
        .filter_level(match args {
            _ if args.trace => LevelFilter::Trace,
            _ if args.debug => LevelFilter::Debug,
            _ if args.verbose => LevelFilter::Info,
            _ if args.quiet => LevelFilter::Error,
            _ if args.silent => LevelFilter::Off,
            #[cfg(debug_assertions)]
            _ => LevelFilter::Trace,
            #[cfg(not(debug_assertions))]
            _ => LevelFilter::Error,
        })
        .build();
    LogWrapper::new(config::MULTIPROG.lock().unwrap().clone(), logger)
        .try_init()
        .expect("Failed to initialize logger!");
    info!("logger started!");
    proc_args();
    info!("Welcome to the APCS Homework tester!");
    match &args.mode {
        CommandType::Init => {
            info!("creating bare config file...");
            let mut f = File::create("config.toml").unwrap();
            let buf = toml::to_string_pretty(&Config::default()).unwrap();
            f.write(buf.as_bytes()).expect("failed to write to config!");
            exit(0);
        }
        _ => {}
    };
    let config = &CONFIG;
    debug!("Config:\n{}", (*config).clone());
    let target = unpacker::unpack_dir(CONFIG.target.clone()).await;
    debug!("Starting safety checks...");
    debug!(
        "checking: {:?}",
        target
            .iter()
            .cloned()
            .filter_map(|el| el.ok())
            .collect::<Vec<_>>()
    );
    let check_result: HashMap<PathBuf, Vec<IllegalExpr>> =
        check_dirs(target.iter().cloned().filter_map(|el| el.ok()).collect())
            .await
            .unwrap()
            .iter()
            .filter(|el| !el.1.is_empty())
            .map(|el| (el.0.clone(), el.1.clone()))
            .collect();
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
    let mut exec: HashSet<PathBuf> = TEMPDIR
        .read_dir()
        .unwrap()
        .map(|el| PathBuf::from(el.unwrap().path()))
        .collect();
    for i in check_result {
        exec.remove(&i.0);
    }
    info!("Testing...");
    debug!("Target dirs: {:?}", exec);
    let res = test::test_dirs(exec).await;
    info!("{:#?}", res);
    if !SIMPLEOPTS.artifacts {
        info!("cleaning up...");
        remove_dir_all(TEMPDIR.clone()).await.unwrap();
    }
}
