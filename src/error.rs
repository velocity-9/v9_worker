use std::fmt::{self, Display, Formatter};
use std::str::Utf8Error;

use hyper::{Body, Response};

#[derive(Debug, Fail)]
pub enum WorkerError {
    Hyper(hyper::error::Error),
    InternalJsonHandling(serde_json::Error),
    InvalidUtf8(Utf8Error),
    PathNotFound(String),
    WrongMethod,
}

impl Display for WorkerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        // Technically this isn't very DRY, but I felt like factoring it out hurt readability YMMV
        match self {
            WorkerError::Hyper(e) => {
                f.write_str("WorkerError, caused by internal hyper error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerError::InvalidUtf8(e) => {
                f.write_str("WorkerError, caused by internal utf8 decode error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerError::PathNotFound(path) => {
                f.write_str("WorkerError, path not found (")?;
                f.write_str(path)?;
                f.write_str(")")?;
            }

            WorkerError::InternalJsonHandling(e) => {
                f.write_str("WorkerError, caused by internal serde_json error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerError::WrongMethod => {
                f.write_str("WorkerError, invalid http verb")?;
            }
        }
        Ok(())
    }
}

impl Into<Response<Body>> for WorkerError {
    fn into(self) -> Response<Body> {
        match self {
            // Special case the "PathNotFound" error, since it maps cleanly to a 404
            WorkerError::PathNotFound(_) => {
                Response::builder().status(404).body(Body::from("")).unwrap()
            }

            // Also special case the "WrongMethodError" error since it maps cleanly to a 405
            WorkerError::WrongMethod => Response::builder().status(405).body(Body::from("")).unwrap(),

            // Otherwise a 500 response is fine
            e => Response::builder()
                .status(500)
                .body(Body::from(e.to_string()))
                .unwrap(),
        }
    }
}

impl From<hyper::error::Error> for WorkerError {
    fn from(e: hyper::error::Error) -> Self {
        WorkerError::Hyper(e)
    }
}

impl From<Utf8Error> for WorkerError {
    fn from(e: Utf8Error) -> Self {
        WorkerError::InvalidUtf8(e)
    }
}

impl From<serde_json::Error> for WorkerError {
    fn from(e: serde_json::Error) -> Self {
        WorkerError::InternalJsonHandling(e)
    }
}
