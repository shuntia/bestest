#![deny(clippy::all)]
use console::style;
use indicatif_log_bridge::LogWrapper;
use log::LevelFilter;
#[expect(unused)]
use log::{debug, error, info, trace, warn};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
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
use anyhow::Result;
use checker::{IllegalExpr, check_dirs};
use config::{CONFIG, CommandType, ConfigParams, SIMPLEOPTS, TEMPDIR, proc_args};

use crate::config::ARGS;

#[expect(clippy::unwrap_used)]
#[tokio::main]
async fn main() -> Result<()> {
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
    #[expect(clippy::expect_used)]
    LogWrapper::new(config::MULTIPROG.lock().await.clone(), logger)
        .try_init()
        .expect("Failed to initialize logger!");
    debug!("logger started!");
    proc_args();
    info!("Welcome to the APCS Homework tester!");
    #[cfg(feature = "gui")]
    {
        tokio::task::spawn_blocking(gui::init);
        gui::app::wait_for_config();
    }
    info!("{:?}", ARGS);
    info!("{args:?}");
    match args.mode {
        CommandType::Init => {
            info!("creating bare config file...");
            let mut f = File::create("config.toml").await.unwrap();
            let buf = toml::to_string_pretty(&ConfigParams::default()).unwrap();
            #[expect(clippy::expect_used)]
            f.write_all(buf.as_bytes())
                .await
                .expect("failed to write to config!");
            return Ok(());
        }
        CommandType::Run => run().await,
        CommandType::Test | CommandType::Format => {
            todo!("Test and format are not yet implemented!")
        }
    }
}

async fn run() -> Result<()> {
    let config = &CONFIG;
    debug!("Config:\n{}", (*config).clone());
    let target = unpacker::unpack_dir(CONFIG.target.clone()).await;
    if target.is_empty() {
        error!("Failed to unpack files. Are you sure the Regex and file format is correct?");
        return Ok(());
    }
    info!("Starting safety checks...");
    debug!(
        "checking: {:?}",
        target
            .iter()
            .cloned()
            .filter_map(Result::ok)
            .collect::<Vec<_>>()
    );
    let check_result: HashMap<PathBuf, Vec<IllegalExpr>> =
        check_dirs(target.iter().cloned().filter_map(Result::ok).collect())
            .await
            .unwrap()
            .iter()
            .filter(|el| !el.1.is_empty())
            .map(|el| (el.0.clone(), el.1.clone()))
            .collect();
    if check_result.is_empty() {
        info!("{} All safety checks passed.", style("[AC]").green().bold());
    } else {
        warn!("Potentially dangerous code detected.");
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
        while let Some(s) = rem.parent() {
            if s.to_path_buf() != *TEMPDIR {
                rem = rem.parent().unwrap().to_path_buf();
            }
        }
        exec.remove(&rem);
    }
    info!("Starting tests...");
    debug!("Target dirs: {exec:?}");
    if exec.is_empty() {
        error!("None passed the safety test. Did you configure your safety settings correctly?");
        return Ok(());
    }
    let res = test::test_dirs(exec).await;
    debug!("Results: {res:#?}");
    let mut points = vec![];
    for i in res {
        let mut acc = 0;
        for j in 0..i.1.len() {
            #[expect(clippy::indexing_slicing)]
            if i.1[j].is_correct() {
                acc += config.testcases[j].points;
            }
        }
        #[expect(clippy::unwrap_used)]
        points.push((i.0.file_name().unwrap().to_str().unwrap().to_owned(), acc));
    }
    if let Some(s) = SIMPLEOPTS.output.clone() {
        let mut file = File::create(s).await?;
        #[expect(clippy::expect_used)]
        for i in points {
            file.write_all(&format!("{}: {}\n", i.0, i.1).into_bytes())
                .await
                .expect("Failed to write to result file!");
        }
    } else {
        #[expect(clippy::print_stdout)]
        for i in points {
            println!("{}: {}", i.0, i.1);
        }
    }
    #[cfg(not(feature = "gui"))]
    if !SIMPLEOPTS.artifacts {
        debug!("cleaning up...");
        remove_dir_all(TEMPDIR.clone()).await?;
    }
    #[cfg(feature = "gui")]
    {
        debug!("cleaning up...");
        remove_dir_all(TEMPDIR.clone()).await?;
    }

    Ok(())
}
