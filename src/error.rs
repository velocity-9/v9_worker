use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::num::TryFromIntError;
use std::str::Utf8Error;
use std::string::FromUtf8Error;

use failure::Backtrace;
use hyper::{Body, Response, StatusCode};
use subprocess::{ExitStatus, PopenError};
use tokio::task::JoinError;

// TODO: Add `type WorkerResult<V> = Result<V, WorkerError>`, and use that everywhere

#[derive(Debug)]
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

impl Error for WorkerError {}

impl From<WorkerErrorKind> for WorkerError {
    fn from(kind: WorkerErrorKind) -> Self {
        Self::new(kind)
    }
}

#[derive(Debug)]
pub enum WorkerErrorKind {
    Docker(ExitStatus, String, String),
    Hyper(hyper::error::Error),
    Io(io::Error),
    IntegerConversion(TryFromIntError),
    InternalJsonHandling(serde_json::Error),
    InvalidSerialization(&'static str, Vec<u8>),
    InvalidUtf8(Utf8Error),
    Nix(nix::Error),
    OperationTimedOut(&'static str),
    OsStringConversion(OsString),
    PathNotFound(String),
    PipeDisconnected,
    Regex(regex::Error),
    SubprocessStart(PopenError),
    SubprocessTerminated(ExitStatus),
    TokioJoinError(JoinError),
    UnsupportedPlatform(&'static str),
    WrongMethod,
}

impl Display for WorkerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        match &self.kind {
            WorkerErrorKind::Docker(exit_status, stdout, stderr) => {
                write!(
                    f,
                    "WorkerError, caused by internal Docker error: exit_status = {:?}, output = ({}, {})",
                    exit_status,
                    stdout,
                    stderr
                )?;
            }

            WorkerErrorKind::Hyper(e) => {
                write!(f, "WorkerError, caused by internal hyper error: {}", e)?;
            }

            WorkerErrorKind::Io(e) => {
                write!(f, "WorkerError, caused by internal I/O error: {}", e)?;
            }

            WorkerErrorKind::IntegerConversion(e) => {
                write!(
                    f,
                    "WorkerError, caused by internal integer conversion error: {}",
                    e
                )?;
            }

            WorkerErrorKind::InternalJsonHandling(e) => {
                write!(f, "WorkerError, caused by internal serde_json error: {}", e)?;
            }

            WorkerErrorKind::InvalidSerialization(problem, l) => {
                write!(
                    f,
                    "WorkerError, {} with invalid series of bytes: {:?}",
                    problem, l
                )?;
            }

            WorkerErrorKind::InvalidUtf8(e) => {
                write!(f, "WorkerError, caused by internal utf8 decode error: {}", e)?;
            }

            WorkerErrorKind::Nix(e) => {
                write!(f, "WorkerError, caused by internal unix error: {}", e)?;
            }

            WorkerErrorKind::OperationTimedOut(op_name) => {
                write!(f, "WorkerError, {} operation timed out", *op_name)?;
            }

            WorkerErrorKind::OsStringConversion(os_string) => {
                write!(f, "WorkerError, caused by problematic OsString ({:?})", os_string)?;
            }

            WorkerErrorKind::PathNotFound(path) => {
                write!(f, "WorkerError, path not found: {}", path)?;
            }

            WorkerErrorKind::PipeDisconnected => {
                write!(f, "Worker Error, internal pipe disconnected")?;
            }

            WorkerErrorKind::Regex(e) => {
                write!(f, "Worker Error, invalid regex: {}", e)?;
            }

            WorkerErrorKind::SubprocessStart(e) => {
                write!(f, "WorkerError, caused by internal subprocess error: {}", e)?;
            }

            WorkerErrorKind::SubprocessTerminated(exit_status) => {
                write!(
                    f,
                    "WorkerError, caused by subprocess terminating, with code {:?}",
                    exit_status
                )?;
            }

            WorkerErrorKind::TokioJoinError(e) => {
                write!(f, "WorkerError, caused by internal tokio join error: {}", e)?;
            }

            WorkerErrorKind::UnsupportedPlatform(plat) => {
                write!(f, "WorkerError, unsupported platform: {}", plat)?;
            }

            WorkerErrorKind::WrongMethod => {
                write!(f, "WorkerError, invalid http verb")?;
            }
        }
        Ok(())
    }
}

impl Into<Response<Body>> for WorkerError {
    fn into(self) -> Response<Body> {
        match &self.kind {
            // Special case the "PathNotFound" error, since it maps cleanly to a 404
            // IMPORTANT: The 404 message here is part of our API
            // DO NOT CHANGE without modifying the router
            WorkerErrorKind::PathNotFound(_) => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("v9: worker 404"))
                .unwrap(),

            // Also special case the "WrongMethodError" error since it maps cleanly to a 405
            WorkerErrorKind::WrongMethod => Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(Body::from(""))
                .unwrap(),

            // Otherwise a 543 response is what the spec demands
            _ => Response::builder()
                .status(543)
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

impl From<FromUtf8Error> for WorkerError {
    fn from(e: FromUtf8Error) -> Self {
        WorkerErrorKind::InvalidUtf8(e.utf8_error()).into()
    }
}

impl From<nix::Error> for WorkerError {
    fn from(e: nix::Error) -> Self {
        WorkerErrorKind::Nix(e).into()
    }
}

impl From<regex::Error> for WorkerError {
    fn from(e: regex::Error) -> Self {
        WorkerErrorKind::Regex(e).into()
    }
}

impl From<PopenError> for WorkerError {
    fn from(e: PopenError) -> Self {
        WorkerErrorKind::SubprocessStart(e).into()
    }
}

impl From<JoinError> for WorkerError {
    fn from(e: JoinError) -> Self {
        WorkerErrorKind::TokioJoinError(e).into()
    }
}
