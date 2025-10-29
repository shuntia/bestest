use crate::config::Orderby;
use crate::config::{CONFIG, KNOWN_EXTENSIONS, MULTIPROG, TEMPDIR, generate_regex};
use core::time::Duration;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, trace, warn};
use std::fs::{self, File};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{copy, create_dir};
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use walkdir::WalkDir;
use zip::read::ZipArchive;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum UnpackError {
    FileFormat,
    Executable,
    FileType,
    ZipProblem(String),
    Os(i32),
    Ignore,
    Unknown,
}
fn unzip_to_dir<T: AsRef<Path> + Clone>(zip_path: T, dest_dir: T) -> zip::result::ZipResult<()> {
    let zip_file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(zip_file)?;

    if !dest_dir.as_ref().exists() {
        fs::create_dir_all(dest_dir.clone())?;
    }

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.name().to_owned();
        let dest_path = dest_dir.as_ref().join(file_name);

        if file.name().ends_with('/') {
            fs::create_dir_all(&dest_path)?;
        } else {
            let mut out_file = File::create(dest_path)?;
            std::io::copy(&mut file, &mut out_file)?;
        }
    }

    Ok(())
}

pub async fn unpack_dir(p: PathBuf) -> Vec<Result<PathBuf, UnpackError>> {
    let max_threads = match usize::try_from(CONFIG.threads) {
        Ok(value) => value,
        Err(_) => {
            warn!("Thread count exceeds usize::MAX; capping to usize::MAX");
            usize::MAX
        }
    };
    let max_threads = max_threads.max(1);
    let semaphore = Arc::new(Semaphore::new(max_threads));
    let mut handles = vec![];
    if p.is_file() {
        warn!(
            "Expected a directory to unpack, received a file instead ({}). Treating it as a single submission.",
            p.display()
        );
        return vec![unpack(p).await];
    }
    debug!("unpacking...");
    let entries: Vec<PathBuf> = match p.read_dir() {
        Ok(read_dir) => read_dir
            .filter_map(|entry| match entry {
                Ok(dir_entry) => Some(dir_entry.path()),
                Err(e) => {
                    warn!("Failed to read directory entry while unpacking: {e}");
                    None
                }
            })
            .collect(),
        Err(e) => {
            error!("Failed to read directory {:?}: {e}", p);
            return vec![Err(UnpackError::Os(e.raw_os_error().unwrap_or(-1)))];
        }
    };
    let mp = MULTIPROG.lock().await;
    let op = Arc::new(Mutex::new(mp.add(ProgressBar::new(entries.len() as u64))));
    op.lock()
        .await
        .enable_steady_tick(Duration::from_millis(50));
    for entry_path in entries {
        let prog = mp.add(ProgressBar::new_spinner());
        let spinner_style = ProgressStyle::default_spinner()
            .template("{spinner} Unpacking {msg}")
            .unwrap_or_else(|err| {
                warn!("Failed to configure unpack spinner style: {err}");
                ProgressStyle::default_spinner()
            })
            .tick_strings(&crate::config::SPINNER);
        prog.set_style(spinner_style);
        prog.enable_steady_tick(Duration::from_millis(50));
        handles.push(tokio::task::spawn(unpack_semaphore_prog(
            entry_path,
            Arc::clone(&semaphore),
            prog,
            Arc::clone(&op),
        )));
    }
    drop(mp);
    let mut ret = vec![];
    for i in handles {
        if let Ok(result) = i.await {
            ret.push(result);
            if let Some(last) = ret.last() {
                match last {
                    Ok(path) => {
                        let name = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("<unknown>");
                        debug!("Finished unpacking {}", name);
                    }
                    Err(e) => match e {
                        UnpackError::Ignore => {}
                        err @ (UnpackError::FileFormat
                        | UnpackError::Executable
                        | UnpackError::FileType
                        | UnpackError::ZipProblem(_)
                        | UnpackError::Os(_)
                        | UnpackError::Unknown) => error!("Failed to unpack: {err:?}"),
                    },
                }
            }
        }
    }
    op.lock().await.finish_and_clear();
    debug!("All unpacks complete.");
    ret
}

async fn unpack_semaphore_prog(
    p: PathBuf,
    s: Arc<Semaphore>,
    pr: ProgressBar,
    op: Arc<Mutex<ProgressBar>>,
) -> Result<PathBuf, UnpackError> {
    let ret = unpack_semaphore(p.clone(), s).await;
    op.lock().await.inc(1);
    pr.finish_and_clear();
    debug!("Completed {}", p.to_string_lossy());
    ret
}

async fn unpack_semaphore(p: PathBuf, s: Arc<Semaphore>) -> Result<PathBuf, UnpackError> {
    let sp = match s.acquire().await {
        Ok(permit) => permit,
        Err(e) => {
            error!("Semaphore closed while unpacking: {e}");
            return Err(UnpackError::Unknown);
        }
    };
    let ret = unpack(p).await;
    drop(sp);
    ret
}

pub async fn unpack(p: PathBuf) -> Result<PathBuf, UnpackError> {
    if p.is_dir() {
        warn!(
            "Unpacker received directory {}; leaving it untouched.",
            p.display()
        );
        return Err(UnpackError::Ignore);
    }
    if p.is_file()
        && !p
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| KNOWN_EXTENSIONS.contains(ext))
            .unwrap_or(false)
    {
        debug!("Skipping file with unsupported extension: {}", p.display());
        return Err(UnpackError::Ignore);
    }
    let regex = match generate_regex(&CONFIG.format) {
        Ok(regex) => regex,
        Err(e) => {
            error!("Failed to compile format regex {}: {e}", CONFIG.format);
            return Err(UnpackError::Unknown);
        }
    };
    let Some(file_name) = p.file_name().and_then(|name| name.to_str()) else {
        debug!("Unable to read filename for {}; skipping", p.display());
        return Err(UnpackError::Ignore);
    };
    let name;
    if let Some(caps) = regex.captures(file_name) {
        match caps.name(match CONFIG.orderby {
            Orderby::Name => "name",
            Orderby::Id => "id",
        }) {
            Some(s) => name = s,
            None => {
                error!("format requires {{name}} or {{id}} so that bestest knows what to do!");
                error!("Failed to capture {:?} for {p:?}", CONFIG.orderby);
                return Err(UnpackError::FileFormat);
            }
        }
        let ext = if let Some(ext) = caps.name("extension") {
            ext.as_str().to_owned()
        } else if let Some(ext_str) = p.extension().and_then(|ext| ext.to_str()) {
            ext_str.to_owned()
        } else {
            warn!(
                "Failed to determine file extension for {}; attempting fallback behaviour.",
                p.display()
            );
            #[cfg(target_os = "windows")]
            {
                error!(
                    "Unable to determine file extension on Windows for {:?}. Skipping.",
                    p
                );
                return Err(UnpackError::FileFormat);
            }
            #[cfg(unix)]
            {
                match p.metadata() {
                    Ok(metadata) => {
                        if metadata.permissions().mode() & 0o111 != 0 {
                            error!(
                                "Received an executable file! Direct execution is not supported."
                            );
                            return Err(UnpackError::Executable);
                        }
                        error!("Not executable nor of a known file type!");
                        return Err(UnpackError::FileType);
                    }
                    Err(e) => {
                        error!("Failed to read metadata for {:?}: {e}", p);
                        return Err(UnpackError::Unknown);
                    }
                }
            }
        };
        if ["toml", "json"].contains(&ext.as_str()) {
            return Err(UnpackError::Ignore);
        }
        let target = TEMPDIR.clone().join(name.as_str());
        match create_dir(&target).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(UnpackError::Os(e.raw_os_error().unwrap_or(-1)));
            }
        }
        if ["zip", "tar", "tar.gz"].contains(&ext.as_str()) {
            match unzip_to_dir(p, target.clone()) {
                Ok(()) => {}
                Err(e) => {
                    return Err(UnpackError::ZipProblem(e.to_string()));
                }
            }
        } else {
            let original_ext = p
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("");
            match copy(
                p.clone(),
                target.join(
                    caps.name("filename")
                        .map_or_else(|| name.as_str(), |s| s.as_str())
                        .to_owned()
                        + "."
                        + original_ext,
                ),
            )
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(UnpackError::Os(e.raw_os_error().unwrap_or(-1))),
            }
        }
        return Ok(target);
    }
    trace!(
        "Skipping file {} because it did not match configured format {}",
        p.display(),
        CONFIG.format
    );
    Err(UnpackError::Ignore)
}

#[must_use]
pub fn find_in_dir(p: &PathBuf, target: &str) -> Option<PathBuf> {
    let target_lower = target.to_lowercase();
    for entry in WalkDir::new(p) {
        match entry {
            Ok(dir_entry) => {
                let name = match dir_entry.file_name().to_str() {
                    Some(name) => name.to_lowercase(),
                    None => continue,
                };
                if name.contains(&target_lower) {
                    return Some(dir_entry.into_path());
                }
            }
            Err(e) => warn!("Failed to walk directory while probing entry: {e}"),
        }
    }
    None
}
