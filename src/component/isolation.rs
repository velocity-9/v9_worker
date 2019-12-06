use std::fmt::Debug;
use std::time::{Duration, Instant};

use subprocess::{Popen, PopenConfig};

use crate::docker::idle_container_creator::{get_idle_container, CODE_FOLDER};
use crate::docker::{load_docker_image, V9Container};
use crate::error::{WorkerError, WorkerErrorKind};
use crate::fs_utils::canonicalize;
use crate::model::{ActivateRequest, ExecutionMethod};
use crate::named_pipe::NamedPipe;

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
        // We do not validate whether "ar.executable_file" is a valid path here
        // It's better for each isolation controller to deal with it individually, since they need
        // to account for the edge case (it becoming invalid) anyway
        let isolation_controller: Box<dyn ProcessIsolationController> = match ar.execution_method {
            ExecutionMethod::ContainerizedScript => {
                Box::new(ContainerizedScriptController::new(ar.executable_file)?)
            }
            ExecutionMethod::DockerArchive => {
                Box::new(DockerArchiveController::new(&ar.executable_file)?)
            }
            ExecutionMethod::PythonUnsafe => Box::new(PythonUnsafeController::new(ar.executable_file)?),
        };

        // If we want to start the process automatically, we can use this code. But it makes testing cold starts hard

        // let process = isolation_controller.boot_process();
        // if let Err(e) = &process {
        //    warn!("Could not automatically start the component", e)
        // }

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
        if !cfg!(target_os = "linux") {
            return Err(WorkerErrorKind::UnsupportedPlatform("must be linux!").into());
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

        let container = V9Container::start(pipe, &self.docker_image_tag, &[&c_in, &c_out])?;

        Ok(Box::new(ContainerizedProcessHandle {
            container,
            helper_subproccess: None,
        }))
    }
}

#[derive(Debug)]
pub struct ContainerizedScriptController {
    executable_file: String,
}

impl ContainerizedScriptController {
    pub fn new(executable_file: String) -> Result<Self, WorkerError> {
        if !cfg!(target_os = "linux") {
            return Err(WorkerErrorKind::UnsupportedPlatform("must be linux!").into());
        }

        Ok(Self { executable_file })
    }
}

impl ProcessIsolationController for ContainerizedScriptController {
    fn boot_process(&self) -> Result<Box<dyn IsolatedProcessHandle>, WorkerError> {
        let mut container = get_idle_container()?;

        // Copy over the files
        container.copy_directory_in(&self.executable_file, CODE_FOLDER)?;

        let c_in = canonicalize(container.pipe().component_input_file())?;
        let c_out = canonicalize(container.pipe().component_output_file())?;

        let subprocess =
            container.exec_async(&["sh", &format!("{}/{}", CODE_FOLDER, "start.sh"), &c_in, &c_out])?;

        Ok(Box::new(ContainerizedProcessHandle {
            container,
            helper_subproccess: Some(subprocess),
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

#[derive(Debug)]
pub struct ContainerizedProcessHandle {
    container: V9Container,
    // When we're running a containerized script, there is a helper subprocess we need to keep around
    helper_subproccess: Option<Popen>,
}

impl IsolatedProcessHandle for ContainerizedProcessHandle {
    fn query_process(&mut self, req: &str) -> Result<String, WorkerError> {
        // Check if the subprocess has terminated
        if let Some(exit_status) = self.container.process().poll() {
            return Err(WorkerErrorKind::SubprocessTerminated(exit_status).into());
        }

        trace!("Writing {:?} to piped process", req);
        let resp = self.container.pipe().query(req)?;
        trace!("Got back {:?} from piped process", resp);

        Ok(resp)
    }
}

impl Drop for ContainerizedProcessHandle {
    fn drop(&mut self) {
        if let Some(p) = &mut self.helper_subproccess {
            if let Err(e) = p.terminate() {
                // Detach so we don't hang waiting for it
                p.detach();

                warn!("Failed to terminate process {:?}, err {:?}", p, e);
            }
        }
    }
}
