use console::style;
use env_logger;
use indicatif_log_bridge::LogWrapper;
use log::LevelFilter;
#[allow(unused)]
use log::{debug, error, info, trace, warn};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::exit,
};
use tokio::{
    fs::{remove_dir_all, File},
    io::AsyncWriteExt,
};
pub mod checker;
pub mod config;
pub mod executable;
pub mod gui;
pub mod lang;
pub mod test;
pub mod unpacker;
use checker::{check_dirs, IllegalExpr};
use config::*;

#[tokio::main]
async fn main() {
    #[cfg(feature = "gui")]
    {
        tokio::spawn(gui::init().await);
        gui::app::wait_for_config().await;
    }
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
    LogWrapper::new(config::MULTIPROG.lock().await.clone(), logger)
        .try_init()
        .expect("Failed to initialize logger!");
    debug!("logger started!");
    proc_args();
    info!("Welcome to the APCS Homework tester!");
    match &args.mode {
        CommandType::Init => {
            info!("creating bare config file...");
            let mut f = File::create("config.toml").await.unwrap();
            let buf = toml::to_string_pretty(&ConfigParams::default()).unwrap();
            f.write_all(buf.as_bytes())
                .await
                .expect("failed to write to config!");
            exit(0);
        }
        _ => {}
    };
    let config = &CONFIG;
    debug!("Config:\n{}", (*config).clone());
    let target = unpacker::unpack_dir(CONFIG.target.clone()).await;
    if target.is_empty() {
        error!("Failed to unpack files. Are you sure the Regex and file format is correct?");
        exit(0);
    }
    info!("Starting safety checks...");
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
        info!(
            "{} All safety checks passed.",
            style("[AC]").green().bold().to_string()
        );
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
        let mut rem = i.0;
        while rem.parent().unwrap().to_path_buf() != *TEMPDIR {
            rem = rem.parent().unwrap().to_path_buf();
        }
        exec.remove(&rem);
    }
    info!("Starting tests...");
    debug!("Target dirs: {:?}", exec);
    if exec.is_empty() {
        error!("None passed the safety test. Are you sure you can trust your students? If so, configure it in the \"allow\" config within the config file.");
        exit(0);
    }
    let res = test::test_dirs(exec).await;
    debug!("Results: {:#?}", res);
    let mut points = vec![];
    for i in res {
        let mut acc = 0;
        for j in 0..i.1.len() {
            if i.1[j].is_correct() {
                acc += config.testcases[j].points
            }
        }
        points.push((i.0.file_name().unwrap().to_str().unwrap().to_owned(), acc));
    }
    if let Some(s) = &SIMPLEOPTS.output {
        let mut f = File::create(s).await.unwrap();
        for i in points {
            f.write_all(&format!("{}: {}\n", i.0, i.1).into_bytes())
                .await
                .expect("Failed to write to result file!");
        }
    } else {
        for i in points {
            println!("{}: {}", i.0, i.1);
        }
    }
    if !SIMPLEOPTS.artifacts {
        debug!("cleaning up...");
        remove_dir_all(TEMPDIR.clone()).await.unwrap();
    }
}
