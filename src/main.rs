#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::cargo)]
#![deny(clippy::restriction)]
use console::style;
use indicatif_log_bridge::LogWrapper;
use log::LevelFilter;
#[expect(unused)]
use log::{debug, error, info, trace, warn};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::exit,
};
use tokio::{
    fs::{File, remove_dir_all},
    io::AsyncWriteExt as _,
};
pub mod checker;
pub mod config;
pub mod executable;
#[cfg(feature = "gui")]
pub mod gui;
pub mod lang;
pub mod test;
pub mod unpacker;
use checker::{IllegalExpr, check_dirs};
use config::{CONFIG, CommandType, ConfigParams, SIMPLEOPTS, TEMPDIR, proc_args};

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
    LogWrapper::new(config::MULTIPROG.lock().await.clone(), logger)
        .try_init()
        .expect("Failed to initialize logger!");
    debug!("logger started!");
    proc_args();
    info!("Welcome to the APCS Homework tester!");
    #[cfg(feature = "gui")]
    {
        tokio::spawn(gui::init());
        gui::app::wait_for_config().await;
    }
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
        CommandType::Run | CommandType::Test | CommandType::Format => {}
    }
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
            .filter_map(|el| return el.ok())
            .collect::<Vec<_>>()
    );
    let check_result: HashMap<PathBuf, Vec<IllegalExpr>> =
        check_dirs(target.iter().cloned().filter_map(|el| return el.ok()).collect())
            .await
            .unwrap()
            .iter()
            .filter(|el| return !el.1.is_empty())
            .map(|el| return (el.0.clone(), el.1.clone()))
            .collect();
    if check_result.is_empty() {
        info!(
            "{} All safety checks passed.",
            style("[AC]").green().bold()
        );
    } else {
        warn!("Dangerous code detected.");
        for i in &check_result {
            warn!("{i:?}");
        }
        warn!("Aborting check for those files.");
        info!(
            "NOTE: if you want to allow potentially dangerous operations, configure it in config.toml."
        );
    }
    // get the executables and remove dangerous files.
    let mut exec: HashSet<PathBuf> = TEMPDIR
        .read_dir()
        .unwrap()
        .map(|el| el.unwrap().path())
        .collect();
    for i in check_result {
        let mut rem = i.0;
        while rem.parent().unwrap().to_path_buf() != *TEMPDIR {
            rem = rem.parent().unwrap().to_path_buf();
        }
        exec.remove(&rem);
    }
    info!("Starting tests...");
    debug!("Target dirs: {exec:?}");
    if exec.is_empty() {
        error!(
            "None passed the safety test. Are you sure you can trust your students? If so, configure it in the \"allow\" config within the config file."
        );
        exit(0);
    }
    let res = test::test_dirs(exec).await;
    debug!("Results: {res:#?}");
    let mut points = vec![];
    for i in res {
        let mut acc = 0;
        for j in 0..i.1.len() {
            if i.1[j].is_correct() {
                acc += config.testcases[j].points;
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
    #[cfg(not(feature = "gui"))]
    if !SIMPLEOPTS.artifacts {
        debug!("cleaning up...");
        remove_dir_all(TEMPDIR.clone()).await.unwrap();
    }
    #[cfg(feature = "gui")]
    {
        debug!("cleaning up...");
        remove_dir_all(TEMPDIR.clone()).await.unwrap();
    }
}
