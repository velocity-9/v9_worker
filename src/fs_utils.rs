use std::path::Path;

use crate::error::{WorkerError, WorkerErrorKind};

pub fn canonicalize(p: &Path) -> Result<String, WorkerError> {
    Ok(p.canonicalize()?
        .into_os_string()
        .into_string()
        .map_err(WorkerErrorKind::OsStringConversion)?)
}
