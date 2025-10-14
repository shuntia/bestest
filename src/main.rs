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
use anyhow::{Context, Result};
use checker::{IllegalExpr, check_dirs};
use config::{CONFIG, CommandType, ConfigParams, SIMPLEOPTS, TEMPDIR, proc_args};

#[tokio::main]
async fn main() -> Result<()> {
    let raw_args = &*config::ARGS;
    let logger = env_logger::builder()
        .filter_level(match raw_args {
            _ if raw_args.trace => LevelFilter::Trace,
            _ if raw_args.debug => LevelFilter::Debug,
            _ if raw_args.verbose => LevelFilter::Info,
            _ if raw_args.quiet => LevelFilter::Error,
            _ if raw_args.silent => LevelFilter::Off,
            #[cfg(debug_assertions)]
            _ => LevelFilter::Trace,
            #[cfg(not(debug_assertions))]
            _ => LevelFilter::Error,
        })
        .build();
    LogWrapper::new(config::MULTIPROG.lock().await.clone(), logger)
        .try_init()
        .context("Failed to initialize logger")?;
    debug!("logger started!");
    let args = &config::SIMPLEOPTS;
    proc_args();
    info!("Welcome to the APCS Homework tester!");
    #[cfg(feature = "gui")]
    {
        tokio::task::spawn_blocking(gui::init);
        gui::app::wait_for_config();
    }
    match args.mode {
        CommandType::Init => {
            info!("creating bare config file...");
            let mut f = File::create("config.toml")
                .await
                .context("failed to create config.toml")?;
            let buf = toml::to_string_pretty(&ConfigParams::default())
                .context("failed to serialize default config")?;
            f.write_all(buf.as_bytes())
                .await
                .context("failed to write default config")?;
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
            .await?
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
    let mut exec: HashSet<PathBuf> = HashSet::new();
    for entry in TEMPDIR
        .read_dir()
        .context("failed to read temporary directory")?
    {
        let dir_entry = entry.context("failed to iterate temporary directory entry")?;
        exec.insert(dir_entry.path());
    }
    for i in check_result {
        let mut rem = i.0;
        while let Some(parent) = rem.parent() {
            if parent == TEMPDIR.as_path() {
                break;
            }
            rem = parent.to_path_buf();
        }
        exec.remove(&rem);
    }
    info!("Starting tests...");
    debug!("Target dirs: {exec:?}");
    if exec.is_empty() {
        error!("None passed the safety test. Did you configure your safety settings correctly?");
        return Ok(());
    }
    let res = test::test_dirs(exec).await?;
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
        let Some(file_name) =
            i.0.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        else {
            warn!("Skipping entry with invalid filename: {:?}", i.0);
            continue;
        };
        points.push((file_name, acc));
    }
    if let Some(s) = SIMPLEOPTS.output.clone() {
        let mut file = File::create(s).await?;
        for i in points {
            file.write_all(format!("{}: {}\n", i.0, i.1).as_bytes())
                .await
                .context("failed to write to result file")?;
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
