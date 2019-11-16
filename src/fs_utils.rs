use std::path::Path;

use crate::error::{WorkerError, WorkerErrorKind};

pub fn canonicalize(p: &Path) -> Result<String, WorkerError> {
    Ok(p.canonicalize()?
        .into_os_string()
        .into_string()
        .map_err(WorkerErrorKind::OsStringConversion)?)
}

pub fn get_filename(p: &Path) -> Result<String, WorkerError> {
    Ok(p.file_name()
        .ok_or_else(|| WorkerErrorKind::NotAFile(p.to_path_buf()))?
        .to_os_string()
        .into_string()
        .map_err(WorkerErrorKind::OsStringConversion)?)
}
