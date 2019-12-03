use std::fmt::Debug;
use std::time::{Duration, Instant};

use subprocess::{Popen, PopenConfig};

use crate::docker::idle_container_creator::get_idle_container;
use crate::docker::{
    call_docker_async, call_docker_sync, exec_in_container_async, exec_in_container_sync,
    load_docker_image,
};
use crate::error::{WorkerError, WorkerErrorKind};
use crate::fs_utils::canonicalize;
use crate::model::{ActivateRequest, ExecutionMethod};
use crate::named_pipe::NamedPipe;

// Shutdown an unused component after 10 minutes
const EXPIRY_DURATION: Duration = Duration::from_secs(60 * 10);
const CODE_FOLDER: &str = "/home/sl";

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
            ExecutionMethod::ContainerizedScript => {
                Box::new(ContainerizedScriptController::new(ar.executable_file)?)
            }
            ExecutionMethod::DockerArchive => {
                Box::new(DockerArchiveController::new(&ar.executable_file)?)
            }
            ExecutionMethod::PythonUnsafe => Box::new(PythonUnsafeController::new(ar.executable_file)?),
        };

        // TODO: Consider if components should auto-start (I think it would help our demo)

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
    executable_file: String,
}

impl PythonUnsafeController {
    pub fn new(executable_file: String) -> Result<Self, WorkerError> {
        Ok(Self { executable_file })
    }
}

impl ProcessIsolationController for PythonUnsafeController {
    fn boot_process(&self) -> Result<Box<dyn IsolatedProcessHandle>, WorkerError> {
        let pipe = NamedPipe::new()?;

        let c_in = canonicalize(pipe.component_input_file())?;
        let c_out = canonicalize(pipe.component_output_file())?;

        let subprocess = Popen::create(
            &["python3", "-u", &self.executable_file, &c_in, &c_out],
            PopenConfig::default(),
        )?;

        Ok(Box::new(PipedProcessHandle { subprocess, pipe }))
    }
}

#[derive(Debug)]
struct DockerArchiveController {
    docker_image_tag: String,
}

impl DockerArchiveController {
    pub fn new(docker_tar_file_path: &str) -> Result<Self, WorkerError> {
        // TODO: Figure out if this will work on windows
        if cfg!(target_os = "macos") {
            return Err(WorkerErrorKind::UnsupportedPlatform("macos").into());
        }

        Ok(Self {
            docker_image_tag: load_docker_image(docker_tar_file_path)?,
        })
    }
}

impl ProcessIsolationController for DockerArchiveController {
    fn boot_process(&self) -> Result<Box<dyn IsolatedProcessHandle>, WorkerError> {
        let pipe = NamedPipe::new()?;

        let c_in = canonicalize(pipe.component_input_file())?;
        let c_out = canonicalize(pipe.component_output_file())?;

        // We're calling docker run, mounting the input and output pipes, then running the loaded
        // image with their paths as parameters
        // TODO: Factor out the "run" logic to docker/mod.rs
        let docker_subprocess = call_docker_async(&[
            "run",
            "-v",
            &format!("{}:{}", c_in, c_in),
            "-v",
            &format!("{}:{}", c_out, c_out),
            &self.docker_image_tag,
            &c_in,
            &c_out,
        ])?;

        Ok(Box::new(PipedProcessHandle {
            subprocess: docker_subprocess,
            pipe,
        }))
    }
}

// TODO: Add a drop that gets rid of the loaded docker images after we're done with them

#[derive(Debug)]
pub struct ContainerizedScriptController {
    executable_file: String,
}

impl ContainerizedScriptController {
    pub fn new(executable_file: String) -> Result<Self, WorkerError> {
        // TODO: Figure out if this will work on windows
        if cfg!(target_os = "macos") {
            return Err(WorkerErrorKind::UnsupportedPlatform("macos").into());
        }

        Ok(Self { executable_file })
    }
}

impl ProcessIsolationController for ContainerizedScriptController {
    fn boot_process(&self) -> Result<Box<dyn IsolatedProcessHandle>, WorkerError> {
        let container = get_idle_container()?;

        // Create the folder in the container
        exec_in_container_sync(&container.docker_container_name, &["mkdir", "-p", CODE_FOLDER])?;

        // Copy over the files
        // (Paths that end with `/.` tell docker to copy contents)
        // TODO: Factor this out to docker/mod.rs
        let source = format!("{}/.", self.executable_file);
        call_docker_sync(&[
            "cp",
            &source,
            &format!("{}:{}", container.docker_container_name, CODE_FOLDER),
        ])?;

        let container_in = container.container_input_pipe_location(&container.pipe)?;
        let container_out = container.container_output_pipe_location(&container.pipe)?;
        let docker_subprocess = exec_in_container_async(
            &container.docker_container_name,
            &[
                "bash",
                &format!("{}/{}", CODE_FOLDER, "start.sh"),
                &container_in,
                &container_out,
            ],
        )?;

        Ok(Box::new(PipedProcessHandle {
            subprocess: docker_subprocess,
            pipe: container.pipe,
        }))
    }
}

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

        trace!("Writing {:?} to piped process", req);
        let resp = self.pipe.query(req)?;
        trace!("Got back {:?} from piped process", resp);

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
