use crate::config::Orderby;
use crate::config::{CONFIG, KNOWN_EXTENSIONS, MULTIPROG, TEMPDIR, generate_regex};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, warn};
use std::fs::{self, File};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::fs::{copy, create_dir};
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use walkdir::WalkDir;
use zip::read::ZipArchive;

#[derive(Debug, Clone)]
pub enum UnpackError {
    FileFormat,
    Executable,
    FileType,
    ZipProblem(String),
    Os(i32),
    Ignore,
    Unknown,
}
async fn unzip_to_dir<T: AsRef<Path> + Clone>(
    zip_path: T,
    dest_dir: T,
) -> zip::result::ZipResult<()> {
    let zip_file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(zip_file)?;

    if !dest_dir.as_ref().exists() {
        fs::create_dir_all(dest_dir.clone())?;
    }

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.name().to_string();
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
    let semaphore = Arc::new(Semaphore::new(CONFIG.threads as usize));
    let mut handles = vec![];
    if p.is_file() {
        error!("Expected path, instead got file! Unpacking single file anyway...");
        return vec![unpack(p).await];
    }
    debug!("unpacking...");
    let mp = MULTIPROG.lock().await;
    let op = Arc::new(Mutex::new(
        mp.add(ProgressBar::new(p.read_dir().unwrap().count() as u64)),
    ));
    op.lock()
        .await
        .enable_steady_tick(Duration::from_millis(50));
    for entry in p.read_dir().unwrap() {
        let prog = mp.add(ProgressBar::new_spinner());
        prog.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner} Unpacking {msg}")
                .unwrap()
                .tick_strings(&crate::config::SPINNER),
        );
        prog.enable_steady_tick(Duration::from_millis(50));
        handles.push(tokio::task::spawn(unpack_semaphore_prog(
            entry.unwrap().path(),
            semaphore.clone(),
            prog,
            op.clone(),
        )));
    }
    let mut ret = vec![];
    for i in handles {
        if let Ok(p) = i.await {
            ret.push(p);
        } else {
            continue;
        }
        match ret.get(ret.len() - 1).unwrap() {
            Ok(p) => {
                debug!(
                    "Successfully unpacked {}",
                    p.file_name().unwrap().to_str().unwrap()
                )
            }
            Err(e) => match e {
                UnpackError::Ignore => {}
                err => error!("Failed to unpack: {:?}", err),
            },
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
    debug!("Completed {}", p.to_str().unwrap());
    ret
}

async fn unpack_semaphore(p: PathBuf, s: Arc<Semaphore>) -> Result<PathBuf, UnpackError> {
    let sp = s.acquire().await.unwrap();
    let ret = unpack(p).await;
    drop(sp);
    return ret;
}

pub async fn unpack(p: PathBuf) -> Result<PathBuf, UnpackError> {
    if p.is_dir() {
        warn!("Unpacker does not know what to do with unpacked directory! Leaving it untouched!");
        return Err(UnpackError::Ignore);
    }
    if p.is_file() && !KNOWN_EXTENSIONS.contains(&p.extension().unwrap().to_str().unwrap()) {
        debug!("Ignoring unknown file.");
        return Err(UnpackError::Ignore);
    }
    let r = generate_regex(&CONFIG.format);
    let name;
    if let Some(caps) = r.captures(p.file_name().unwrap().to_str().unwrap()) {
        match caps.name(match CONFIG.orderby {
            Orderby::Name => "name",
            Orderby::Id => "id",
        }) {
            Some(s) => name = s,
            None => {
                error!("format requires {{name}} or {{id}} so that apcs-tester knows what to do!");
                error!("Capture failed for {:?}", p);
                return Err(UnpackError::FileFormat);
            }
        }
        let s;
        let ext = if let Some(ext) = caps.name("extension") {
            ext.as_str()
        } else if let Some(ext_os) = p.extension() {
            if let Some(ext_str) = ext_os.to_str() {
                s = ext_str.to_string();
                s.as_str()
            } else {
                warn!("Failed to convert extension to str!");
                return Err(UnpackError::FileFormat);
            }
        } else {
            warn!("Failed to get extension!");
            #[cfg(target_os = "windows")]
            panic!("I don't know what to do!");
            // Check if file is executable
            if p.metadata().unwrap().permissions().mode() & 0o111 != 0 {
                error!("Received an executable file! Running it as is.");
                todo!("Support for direct execution");
            } else {
                error!("Not executable nor of a known file type!");
                return Err(UnpackError::FileType);
            }
        };
        if ["toml", "json"].contains(&ext) {
            return Err(UnpackError::Ignore);
        }
        let mut target = TEMPDIR.clone();
        target.push(name.as_str());
        match create_dir(&target).await {
            Ok(_) => {}
            Err(e) => {
                return Err(UnpackError::Os(e.raw_os_error().unwrap()));
            }
        }
        if vec!["zip", "tar", "tar.gz"].contains(&ext) {
            match unzip_to_dir(p, target.clone()).await {
                Ok(_) => {}
                Err(e) => {
                    return Err(UnpackError::ZipProblem(e.to_string()));
                }
            }
        } else {
            match copy(
                p.clone(),
                target.join(
                    match caps.name("filename") {
                        Some(s) => s.as_str(),
                        None => name.as_str(),
                    }
                    .to_owned()
                        + "."
                        + p.extension().unwrap().to_str().unwrap(),
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
    debug!("Regex capture failed! Skipping file.");
    Err(UnpackError::Ignore)
}

pub fn find_in_dir(p: &PathBuf, target: &str) -> Option<PathBuf> {
    for e in WalkDir::new(p).into_iter() {
        if e.as_ref()
            .unwrap()
            .file_name()
            .to_str()
            .unwrap()
            .to_lowercase()
            .contains(&target.to_lowercase())
        {
            return Some(e.unwrap().into_path());
        }
    }
    None
}
