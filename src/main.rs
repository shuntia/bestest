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
mod report;
pub mod test;
pub mod unpacker;
use anyhow::{Context, Result};
use checker::{IllegalExpr, check_dirs};
use config::{CONFIG, CommandType, ConfigParams, SIMPLEOPTS, TEMPDIR, proc_args};
use report::{
    RunReport, TotalsSummary, UnpackSummary, detect_output_format, serialize_report,
    summarize_security, summarize_submissions,
};

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
    let mut unpacked = Vec::new();
    let mut ignored = 0usize;
    let mut failed = 0usize;
    for entry in &target {
        match entry {
            Ok(path) => unpacked.push(path.clone()),
            Err(unpacker::UnpackError::Ignore) => ignored += 1,
            Err(_) => failed += 1,
        }
    }
    info!(
        "Unpacking finished: {} prepared, {} skipped, {} failed.",
        unpacked.len(),
        ignored,
        failed
    );
    if unpacked.is_empty() {
        error!("No submissions were ready after unpacking. Aborting run.");
        return Ok(());
    }
    info!("Starting safety checks...");
    debug!("checking: {:?}", unpacked);
    let check_result: HashMap<PathBuf, Vec<IllegalExpr>> = check_dirs(unpacked.clone())
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
    let security_summary = summarize_security(&check_result);
    let flagged_paths: Vec<PathBuf> = check_result.keys().cloned().collect();
    // get the executables and remove dangerous files.
    let mut exec: HashSet<PathBuf> = HashSet::new();
    for entry in TEMPDIR
        .read_dir()
        .context("failed to read temporary directory")?
    {
        let dir_entry = entry.context("failed to iterate temporary directory entry")?;
        exec.insert(dir_entry.path());
    }
    for path in flagged_paths {
        let mut rem = path.clone();
        while let Some(parent) = rem.parent() {
            if parent == TEMPDIR.as_path() {
                break;
            }
            rem = parent.to_path_buf();
        }
        exec.remove(&rem);
    }
    let total_points_available: u64 = config.testcases.iter().map(|tc| tc.points).sum();
    info!("Starting tests...");
    debug!("Target dirs: {exec:?}");
    if exec.is_empty() {
        error!("None passed the safety test. Did you configure your safety settings correctly?");
        return Ok(());
    }
    let res = test::test_dirs(exec).await?;
    debug!("Results: {res:#?}");
    let (mut submission_reports, mut scoreboard, test_totals) =
        summarize_submissions(res, config, total_points_available);
    if SIMPLEOPTS.sort {
        submission_reports.sort_by(|a, b| a.name.cmp(&b.name));
        scoreboard.sort_by(|a, b| a.0.cmp(&b.0));
    }
    let totals_summary = TotalsSummary {
        submissions: submission_reports.len(),
        submissions_with_issues: test_totals.submissions_with_issues,
        perfect_scores: test_totals.perfect_scores,
        max_points_per_submission: total_points_available,
        cases_total: test_totals.total_cases,
        cases_passed: test_totals.passed_cases,
    };
    let run_report = RunReport {
        unpack: UnpackSummary {
            prepared: unpacked.len(),
            skipped: ignored,
            failed,
        },
        totals: totals_summary,
        security: security_summary,
        submissions: submission_reports,
    };
    info!(
        "Scoring complete for {} submission(s); max points per submission: {}.",
        run_report.totals.submissions, run_report.totals.max_points_per_submission
    );
    if run_report.totals.perfect_scores > 0 {
        info!(
            "{} submission(s) achieved full marks.",
            run_report.totals.perfect_scores
        );
    }
    if let Some(path) = SIMPLEOPTS.output.clone() {
        let (format, recognized) = detect_output_format(&path);
        if !recognized {
            if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                warn!("Unsupported output extension `{ext}`; defaulting to plaintext.");
            } else {
                warn!("Output path missing extension; defaulting to plaintext.");
            }
        }
        let mut file = File::create(&path)
            .await
            .with_context(|| format!("failed to create {}", path.display()))?;
        let payload =
            serialize_report(&run_report, format).context("failed to serialize results")?;
        file.write_all(&payload)
            .await
            .context("failed to write results")?;
        info!("Results written to {}", path.display());
    } else {
        #[expect(clippy::print_stdout)]
        for (name, score) in &scoreboard {
            println!("{name}: {score}");
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
