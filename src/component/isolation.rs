use std::fmt::Debug;
use std::fs::canonicalize;
use std::time::{Duration, Instant};

use regex::Regex;
use subprocess::{Exec, Popen, PopenConfig, Redirection};

use crate::error::{WorkerError, WorkerErrorKind};
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
        // TODO: Add validation that the `executable_file` is a valid path to a real file/folder
        let isolation_controller: Box<dyn ProcessIsolationController> = match ar.execution_method {
            ExecutionMethod::PythonUnsafe => Box::new(PythonUnsafeController::new(ar.executable_file)?),
            ExecutionMethod::DockerArchive => {
                Box::new(DockerArchiveController::new(&ar.executable_file)?)
            }
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
        trace!("attempted to query some process and got {:?}", resp);

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

        let c_in = canonicalize(pipe.component_input_file())?
            .into_os_string()
            .into_string()
            .map_err(WorkerErrorKind::OsStringConversion)?;
        let c_out = canonicalize(pipe.component_output_file())?
            .into_os_string()
            .into_string()
            .map_err(WorkerErrorKind::OsStringConversion)?;

        let subprocess = Popen::create(
            &["python3", "-u", &self.executable_file, &c_in, &c_out],
            PopenConfig::default(),
        )?;

        Ok(Box::new(PipedProcessHandle { subprocess, pipe }))
    }
}

#[derive(Debug)]
struct DockerArchiveController {
    pipe_creator: NamedPipeCreator,
    docker_image_tag: String,
}

impl DockerArchiveController {
    pub fn new(docker_tar_file_path: &str) -> Result<Self, WorkerError> {
        if cfg!(target_os = "macos") {
            warn!("using docker for isolation on macOS likely will not work!!!");
        }

        // we are calling docker load, with quiet mode enabled to suppress exccess output
        let argv = &["load", "-q", "--input", docker_tar_file_path];
        debug!("Calling docker argv = {:?}", argv);
        let load_result = Exec::cmd("docker")
            .args(argv)
            .stdout(Redirection::Pipe)
            .stderr(Redirection::Pipe)
            .capture()?;
        let load_exit_status = load_result.exit_status;
        let load_stdout = String::from_utf8(load_result.stdout)?;
        let load_stderr = String::from_utf8(load_result.stderr)?;

        if !load_exit_status.success() {
            return Err(
                WorkerErrorKind::Docker(load_result.exit_status, load_stdout, load_stderr).into(),
            );
        }

        let regex = Regex::new("Loaded image: (?P<tag>.*)\n")?;
        let tag = regex
            .captures(&load_stdout)
            .and_then(|captures| captures.name("tag"))
            .map_or_else(
                || {
                    Err(WorkerErrorKind::Docker(
                        load_exit_status,
                        load_stdout.clone(),
                        load_stderr,
                    ))
                },
                |tag| Ok(tag.as_str()),
            )?;

        debug!("Loaded image (tag = {:?})", tag);

        Ok(Self {
            pipe_creator: NamedPipeCreator::new()?,
            docker_image_tag: tag.to_string(),
        })
    }
}

impl ProcessIsolationController for DockerArchiveController {
    fn boot_process(&self) -> Result<Box<dyn IsolatedProcessHandle>, WorkerError> {
        let pipe = self.pipe_creator.new_pipe()?;

        let c_in = canonicalize(pipe.component_input_file())?
            .into_os_string()
            .into_string()
            .map_err(WorkerErrorKind::OsStringConversion)?;
        let c_out = canonicalize(pipe.component_output_file())?
            .into_os_string()
            .into_string()
            .map_err(WorkerErrorKind::OsStringConversion)?;

        // We're calling docker run, mounting the input and output pipes, then running the loaded
        // image with their paths as parameters
        let argv = &[
            "docker",
            "run",
            "-v",
            &format!("{}:{}", c_in, c_in),
            "-v",
            &format!("{}:{}", c_out, c_out),
            &self.docker_image_tag,
            &c_in,
            &c_out,
        ];
        debug!("Executing docker argv = {:?}", argv);
        let docker_subprocess = Popen::create(argv, PopenConfig::default())?;

        Ok(Box::new(PipedProcessHandle {
            subprocess: docker_subprocess,
            pipe,
        }))
    }
}

// TODO: Add a drop that gets rid of the loaded docker images after we're done with them

#[derive(Debug)]
pub struct PipedProcessHandle {
    subprocess: Popen,
    pipe: NamedPipe,
}

impl IsolatedProcessHandle for PipedProcessHandle {
    fn query_process(&mut self, req: &str) -> Result<String, WorkerError> {
        // Check if the subprocess has terminated
        if let Some(exit_status) = self.subprocess.poll() {
            return Err(WorkerErrorKind::SubprocessTerminated(exit_status).into());
        }

        debug!("Writing {:?} to piped process", req);
        let resp = self.pipe.query(req)?;
        debug!("Got back {:?} from piped process", resp);

        Ok(resp)
    }
}

impl Drop for PipedProcessHandle {
    fn drop(&mut self) {
        if let Err(e) = self.subprocess.terminate() {
            // Detach so we don't hang waiting for it
            self.subprocess.detach();

            warn!("Failed to terminate process {:?}, err {:?}", self.subprocess, e);
        }
    }
}
