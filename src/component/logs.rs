use std::fs::read_to_string;
use std::mem::replace;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tempfile::NamedTempFile;

use crate::error::WorkerError;
use subprocess::{PopenConfig, Redirection};

static DEDUP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct LogTracker {
    // Tracks when a different log tracker is switched to
    dedup_number: u64,
    policy_handle: Arc<LogPolicy>,
}

impl LogTracker {
    pub fn new() -> Self {
        Self {
            dedup_number: DEDUP_COUNTER.fetch_add(1, Ordering::SeqCst),
            policy_handle: LogPolicy::new_no_op_policy(),
        }
    }

    pub fn create_associated_policy(&mut self) -> Result<Arc<LogPolicy>, WorkerError> {
        let backing_file = NamedTempFile::new()?;
        let associated_policy = Arc::new(LogPolicy::ToFile(backing_file));

        let old_policy = replace(&mut self.policy_handle, associated_policy.clone());
        // Check if the old policy is still in use (this is mostly just for debugging/testing)
        if Arc::strong_count(&old_policy) > 1 {
            warn!(
                "Previous policy is still in use! (all future logs from will be ignored from {:?})",
                old_policy
            );
        }
        self.dedup_number = DEDUP_COUNTER.fetch_add(1, Ordering::SeqCst);

        Ok(associated_policy)
    }

    pub fn get_contents(&mut self) -> (u64, Result<Option<String>, WorkerError>) {
        (
            self.dedup_number,
            if self.policy_handle.is_ignore() {
                Ok(None)
            } else {
                self.policy_handle.get_contents().map(Some)
            },
        )
    }
}

#[derive(Debug)]
pub enum LogPolicy {
    ToFile(NamedTempFile),
    // Literally everywhere you might have a LogPolicy, having an Ignore policy is valid
    // Thus we incorporate it into the struct itself, rather than everyone using `Option<LogPolicy>`
    Ignore,
}

impl LogPolicy {
    pub fn new_no_op_policy() -> Arc<Self> {
        Arc::new(Self::Ignore)
    }

    fn is_ignore(&self) -> bool {
        if let Self::Ignore = self {
            true
        } else {
            false
        }
    }

    pub fn get_contents(&self) -> Result<String, WorkerError> {
        match self {
            Self::ToFile(f) => {
                // We don't use the internal `File`, since that may have a cursor in any location
                Ok(read_to_string(f.path())?)
            }
            Self::Ignore => Ok(String::new()),
        }
    }

    pub fn get_popen_config(&self) -> Result<PopenConfig, WorkerError> {
        Ok(match self {
            Self::ToFile(temp_file) => PopenConfig {
                detached: true,
                stdout: Redirection::File(temp_file.as_file().try_clone()?),
                stderr: Redirection::File(temp_file.as_file().try_clone()?),
                ..PopenConfig::default()
            },
            Self::Ignore => PopenConfig {
                detached: true,
                stdout: Redirection::Pipe,
                stderr: Redirection::Pipe,
                ..PopenConfig::default()
            },
        })
    }
}
