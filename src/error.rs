use std::fmt::{self, Display, Formatter};
use std::io;
use std::num::TryFromIntError;
use std::str::Utf8Error;

use failure::Backtrace;
use hyper::{Body, Response};
use subprocess::PopenError;

#[derive(Debug, Fail)]
pub struct WorkerError {
    kind: WorkerErrorKind,
    backtrace: Backtrace,
}

impl WorkerError {
    pub fn new(kind: WorkerErrorKind) -> Self {
        Self {
            kind,
            backtrace: Backtrace::new(),
        }
    }
}

#[derive(Debug)]
pub enum WorkerErrorKind {
    Hyper(hyper::error::Error),
    Io(io::Error),
    IntegerConversion(TryFromIntError),
    InternalJsonHandling(serde_json::Error),
    InvalidSerialization(Vec<u8>),
    InvalidUtf8(Utf8Error),
    Nix(nix::Error),
    OperationTimedOut,
    PathNotFound(String),
    SubprocessDisconnected,
    SubprocessStart(PopenError),
    WrongMethod,
}

impl Display for WorkerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        // Technically this isn't very DRY, but I felt like factoring it out hurt readability YMMV
        match &self.kind {
            WorkerErrorKind::Hyper(e) => {
                f.write_str("WorkerError, caused by internal hyper error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::Io(e) => {
                f.write_str("WorkerError, caused by internal I/O error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::IntegerConversion(e) => {
                f.write_str("WorkerError, caused by internal integer conversion error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::InternalJsonHandling(e) => {
                f.write_str("WorkerError, caused by internal serde_json error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::InvalidSerialization(l) => {
                // Weird import here, but we need this trait in this scope
                use std::fmt::Debug;

                f.write_str("WorkerError, caused by internal invalid serialization (")?;
                l.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::InvalidUtf8(e) => {
                f.write_str("WorkerError, caused by internal utf8 decode error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::Nix(e) => {
                f.write_str("WorkerError, caused by internal unix error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::OperationTimedOut => {
                f.write_str("WorkerError, operation timed out")?;
            }

            WorkerErrorKind::PathNotFound(path) => {
                f.write_str("WorkerError, path not found (")?;
                f.write_str(path)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::SubprocessDisconnected => {
                f.write_str("WorkerError, caused by subprocess disconnecting")?;
            }

            WorkerErrorKind::SubprocessStart(e) => {
                f.write_str("WorkerError, caused by internal subprocess error (")?;
                e.fmt(f)?;
                f.write_str(")")?;
            }

            WorkerErrorKind::WrongMethod => {
                f.write_str("WorkerError, invalid http verb")?;
            }
        }
        Ok(())
    }
}

impl From<WorkerErrorKind> for WorkerError {
    fn from(kind: WorkerErrorKind) -> Self {
        Self::new(kind)
    }
}

impl Into<Response<Body>> for WorkerError {
    fn into(self) -> Response<Body> {
        match &self.kind {
            // Special case the "PathNotFound" error, since it maps cleanly to a 404
            WorkerErrorKind::PathNotFound(_) => {
                Response::builder().status(404).body(Body::from("")).unwrap()
            }

            // Also special case the "WrongMethodError" error since it maps cleanly to a 405
            WorkerErrorKind::WrongMethod => {
                Response::builder().status(405).body(Body::from("")).unwrap()
            }

            // Otherwise a 500 response is fine
            _ => Response::builder()
                .status(500)
                .body(Body::from(self.to_string()))
                .unwrap(),
        }
    }
}

impl From<hyper::error::Error> for WorkerError {
    fn from(e: hyper::error::Error) -> Self {
        WorkerErrorKind::Hyper(e).into()
    }
}

impl From<io::Error> for WorkerError {
    fn from(e: io::Error) -> Self {
        WorkerErrorKind::Io(e).into()
    }
}

impl From<TryFromIntError> for WorkerError {
    fn from(e: TryFromIntError) -> Self {
        WorkerErrorKind::IntegerConversion(e).into()
    }
}

impl From<serde_json::Error> for WorkerError {
    fn from(e: serde_json::Error) -> Self {
        WorkerErrorKind::InternalJsonHandling(e).into()
    }
}

impl From<Utf8Error> for WorkerError {
    fn from(e: Utf8Error) -> Self {
        WorkerErrorKind::InvalidUtf8(e).into()
    }
}

impl From<nix::Error> for WorkerError {
    fn from(e: nix::Error) -> Self {
        WorkerErrorKind::Nix(e).into()
    }
}

impl From<PopenError> for WorkerError {
    fn from(e: PopenError) -> Self {
        WorkerErrorKind::SubprocessStart(e).into()
    }
}
