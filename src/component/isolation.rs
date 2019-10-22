use std::fmt::Debug;
use std::fs::canonicalize;
use std::time::{Duration, Instant};

use subprocess::{Popen, PopenConfig};

use crate::error::WorkerError;
use crate::model::{ActivateRequest, ExecutionMethod};
use crate::named_pipe::{NamedPipe, NamedPipeCreator};

// Shutdown an unused component after 10 minutes
const EXPIRY_DURATION: Duration = Duration::from_secs(60 * 10);

#[derive(Debug)]
pub struct IsolatedProcessWrapper {
    isolation_controller: Box<dyn ProcessIsolationController>,
    process_handle: Option<Box<dyn IsolatedProcessHandle>>,

    last_accessed: Instant,
}

impl IsolatedProcessWrapper {
    pub fn new(ar: ActivateRequest) -> Result<Self, WorkerError> {
        let isolation_controller = match ar.execution_method {
            ExecutionMethod::PythonUnsafe => Box::new(PythonUnsafeController::new(ar.executable_file)?),
        };

        Ok(Self {
            isolation_controller,
            process_handle: None,

            last_accessed: Instant::now(),
        })
    }

    pub fn query_process(&mut self, req: &str) -> Result<String, WorkerError> {
        self.last_accessed = Instant::now();

        if self.process_handle.is_none() {
            self.process_handle = Some(self.isolation_controller.boot_process()?)
        }

        // This is a safe unwrap, since we just ensured we have a booted proccess
        let handle = self.process_handle.as_mut().unwrap();

        let resp = handle.query_process(req);

        // If querying the process fails, then we need to restart it
        if resp.is_err() {
            self.process_handle = None;
        }

        resp
    }

    // The `heartbeat` function is called periodically
    pub fn heartbeat(&mut self) {
        if self.process_handle.is_none() {
            return;
        }

        if Instant::now() - self.last_accessed > EXPIRY_DURATION {
            debug!("Shutting down unused function {:?}", self.process_handle);
            self.process_handle = None
        }
    }
}

pub trait ProcessIsolationController: Debug + Send {
    fn boot_process(&self) -> Result<Box<dyn IsolatedProcessHandle>, WorkerError>;
}

pub trait IsolatedProcessHandle: Debug + Send {
    fn query_process(&mut self, req: &str) -> Result<String, WorkerError>;
}

#[derive(Debug)]
pub struct PythonUnsafeController {
    pipe_creator: NamedPipeCreator,
    executable_file: String,
}

impl PythonUnsafeController {
    pub fn new(executable_file: String) -> Result<Self, WorkerError> {
        Ok(Self {
            pipe_creator: NamedPipeCreator::new()?,
            executable_file,
        })
    }
}

impl ProcessIsolationController for PythonUnsafeController {
    fn boot_process(&self) -> Result<Box<dyn IsolatedProcessHandle>, WorkerError> {
        let pipe = self.pipe_creator.new_pipe()?;

        let c_in = canonicalize(pipe.component_input_file())?.into_os_string();
        let c_out = canonicalize(pipe.component_output_file())?.into_os_string();

        let subprocess = Popen::create(
            &[
                "python3",
                "-u",
                &self.executable_file,
                &c_in.to_string_lossy(),
                &c_out.to_string_lossy(),
            ],
            PopenConfig::default(),
        )?;

        Ok(Box::new(PythonUnsafeHandle { subprocess, pipe }))
    }
}

#[derive(Debug)]
pub struct PythonUnsafeHandle {
    subprocess: Popen,
    pipe: NamedPipe,
}

impl IsolatedProcessHandle for PythonUnsafeHandle {
    fn query_process(&mut self, req: &str) -> Result<String, WorkerError> {
        debug!("Writing {:?} to python-unsafe process", req);
        self.pipe.write(req.as_bytes())?;
        let bytes = self.pipe.read()?;
        let resp = String::from_utf8(bytes).map_err(|e| e.utf8_error())?;
        debug!("Got back {:?} from python-unsafe process", resp);

        Ok(resp)
    }
}

impl Drop for PythonUnsafeHandle {
    fn drop(&mut self) {
        if let Err(e) = self.subprocess.terminate() {
            debug!("Failed to terminate process {:?}, err {:?}", self.subprocess, e);
        }
    }
}
