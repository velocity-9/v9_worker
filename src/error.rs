use std::fmt::{self, Display, Formatter};
use std::str::Utf8Error;

use hyper::{Body, Response};

#[derive(Debug, Fail)]
pub enum WorkerError {
    HyperError(hyper::error::Error),
    InvalidUtf8Error(Utf8Error),
    MutexPoisonedError,
    PathNotFound(String),
    JsonHandlingError(serde_json::Error),
}

impl Display for WorkerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        // Technically this isn't very DRY, but I felt like factoring it out hurt readability YMMV
        match self {
            WorkerError::HyperError(e) => {
                f.write_str("WorkerError, caused by internal hyper error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerError::InvalidUtf8Error(e) => {
                f.write_str("WorkerError, caused by internal utf8 decode error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerError::MutexPoisonedError => {
                f.write_str("WorkerError, caused by internal mutex poisoning")?;
            }

            WorkerError::PathNotFound(path) => {
                f.write_str("WorkerError, path not found (")?;
                f.write_str(path)?;
                f.write_str(")")?;
            }

            WorkerError::JsonHandlingError(e) => {
                f.write_str("WorkerError, caused by internal serde_json error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }
        }
        Ok(())
    }
}

impl Into<Response<Body>> for WorkerError {
    fn into(self) -> Response<Body> {
        match self {
            // Special case the "PathNotFound" error, since it maps cleanly to a 404
            WorkerError::PathNotFound(_) => Response::builder().status(404).body(Body::from("")).unwrap(),
            // Otherwise a 500 response is fine
            e => Response::builder().status(500).body(Body::from(e.to_string())).unwrap(),
        }
    }
}

impl From<hyper::error::Error> for WorkerError {
    fn from(e: hyper::error::Error) -> Self {
        WorkerError::HyperError(e)
    }
}

impl From<Utf8Error> for WorkerError {
    fn from(e: Utf8Error) -> Self {
        WorkerError::InvalidUtf8Error(e)
    }
}

impl From<serde_json::Error> for WorkerError {
    fn from(e: serde_json::Error) -> Self {
        WorkerError::JsonHandlingError(e)
    }
}
