use crate::config::{generate_regex, CONFIG, TEMPDIR};
use nix::libc::{mkdir, mode_t};
use std::fs::{self, File};
use std::path::Path;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::fs::{create_dir, rename};
use zip::read::ZipArchive;

use log::{error, info, warn};
use zip::result::ZipResult;

pub enum UnpackError {
    FileFormat,
    Executable,
    FileType,
    ZipProblem(ZipResult<()>),
    Unknown,
}
impl From<ZipResult<()>> for UnpackError {
    fn from(value: ZipResult<()>) -> Self {
        Self::ZipProblem(value)
    }
}
fn unzip_to_dir<T: AsRef<Path> + Clone>(zip_path: T, dest_dir: T) -> zip::result::ZipResult<()> {
    // Open the ZIP file
    let zip_file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(zip_file)?;

    // Create the destination directory if it doesn't exist
    if !dest_dir.as_ref().exists() {
        fs::create_dir_all(dest_dir.clone())?;
    }

    // Iterate over each file in the archive
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.name().to_string();
        let dest_path = dest_dir.as_ref().join(file_name);

        // Create the destination file (if it's a directory, just create it)
        if file.name().ends_with('/') {
            fs::create_dir_all(&dest_path)?;
        } else {
            // Create a file and copy the content from the ZIP
            let mut out_file = File::create(dest_path)?;
            std::io::copy(&mut file, &mut out_file)?;
        }
    }

    Ok(())
}

pub fn unpack(p: PathBuf) -> Result<PathBuf, UnpackError> {
    if p.is_dir() {
        warn!("Unpacker does not know what to do with unpacked directory! Leaving it untouched!");
    }
    let r = generate_regex(&CONFIG.format);
    let name;
    if let Some(caps) = r.captures(p.file_name().unwrap().to_str().unwrap()) {
        match caps.name("name") {
            Some(s) => name = s,
            None => {
                info!("name not found in regex! looking for id instead...");
                match caps.name("id") {
                    Some(s) => name = s,
                    None => {
                        error!("format requires {{name}} or {{id}} so that apcs-tester knows what to do!");
                        info!("exiting...");
                        panic!("faulty regex format.");
                    }
                }
            }
        }
        let s;
        let ext = if let Some(ext) = caps.name("ext") {
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
        let mut target = TEMPDIR.clone();
        target.push(name.as_str());
        create_dir(&target);
        if vec!["zip", "tar", "tar.gz"].contains(&ext) {
            unzip_to_dir(p, target.clone());
        } else {
            rename(p.clone(), target.join(p.file_name().unwrap()));
        }
        return Ok(target);
    }
    Err(UnpackError::Unknown)
}
