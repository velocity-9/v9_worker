use std::sync::mpsc::{sync_channel, Receiver};
use std::thread;
use std::time::Duration;

use lazy_static::lazy_static;
use parking_lot::Mutex;
use subprocess::Popen;
use tempfile::TempDir;

use crate::docker::call_docker_async;
use crate::error::{WorkerError, WorkerErrorKind};
use crate::fs_utils::get_filename;
use crate::named_pipe::NamedPipe;

// NOTE: the number of idle containers on the system is CONTAINER_CACHE_CHANNEL_SIZE + CACHE_POPULATOR_COUNT
const CONTAINER_CACHE_CHANNEL_SIZE: usize = 3;
const CACHE_POPULATOR_COUNT: usize = 2;

const CONTAINER_IMAGE_TAG: &str = "python";
// 1000000000 seconds ~= 30 years
const SLEEP_TIME: &str = "1000000000";

const CONTAINER_PIPE_FOLDER: &str = "/home/io";

fn next_container_name() -> String {
    let id: u64 = rand::random();
    format!("v9_{}_{}", CONTAINER_IMAGE_TAG, id)
}

fn sync_create_container() -> Result<Container, WorkerError> {
    let pipe_dir = TempDir::new()?;
    let pipe_path = pipe_dir.path().canonicalize()?;
    let pipe_path_string = pipe_path
        .into_os_string()
        .into_string()
        .map_err(WorkerErrorKind::OsStringConversion)?;
    let pipe_mount = format!("{}:{}", pipe_path_string, CONTAINER_PIPE_FOLDER);

    let desired_name = next_container_name();
    let docker_subprocess = call_docker_async(&[
        "run",
        "-v",
        &pipe_mount,
        "--name",
        &desired_name,
        CONTAINER_IMAGE_TAG,
        "sleep",
        SLEEP_TIME,
    ])?;

    let pipe = NamedPipe::in_dir(pipe_dir)?;

    Ok(Container {
        pipe,
        docker_container_name: desired_name,
        docker_run_process: docker_subprocess,
    })
}

pub struct IdleContainerCreator {
    cache_channel_receiver: Mutex<Receiver<Container>>,
}

impl IdleContainerCreator {
    fn new() -> Self {
        // Create the cache channel
        let (sender, receiver) = sync_channel(CONTAINER_CACHE_CHANNEL_SIZE);

        // Create the populator threads
        for _ in 0..CACHE_POPULATOR_COUNT {
            let sender = sender.clone();
            thread::spawn(move || loop {
                let container = sync_create_container();
                match container {
                    Ok(id) => {
                        let send_res = sender.send(id);
                        if send_res.is_err() {
                            warn!("Idle container cache populator thread disconnected. Terminating...");
                            return;
                        }
                    }
                    Err(e) => {
                        error!("Problem creating a container in a working thread: {}", e);
                        info!("Worker thread sleeping after erorring out...");
                        thread::sleep(Duration::from_secs(10));
                    }
                }
            });
        }

        Self {
            cache_channel_receiver: Mutex::new(receiver),
        }
    }

    fn get_idle_container(&self) -> Result<Container, WorkerError> {
        let cached_container_id = self
            .cache_channel_receiver
            .try_lock()
            .and_then(|chan| chan.try_recv().ok());

        match cached_container_id {
            Some(id) => Ok(id),
            None => sync_create_container(),
        }
    }
}

lazy_static! {
    pub static ref GLOBAL_IDLE_CONTAINER_CREATOR: IdleContainerCreator = { IdleContainerCreator::new() };
}

// TODO: Factor container logic out to docker/mod.rs

pub struct Container {
    pub pipe: NamedPipe,

    pub docker_container_name: String,
    pub docker_run_process: Popen,
}

// TODO: Dropping a `Container` should stop and rm the container

impl Container {
    pub fn container_input_pipe_location(&self, p: &NamedPipe) -> Result<String, WorkerError> {
        let pipe_name = get_filename(p.component_input_file())?;
        Ok(format!("{}/{}", CONTAINER_PIPE_FOLDER, pipe_name))
    }

    pub fn container_output_pipe_location(&self, p: &NamedPipe) -> Result<String, WorkerError> {
        let pipe_name = get_filename(p.component_output_file())?;
        Ok(format!("{}/{}", CONTAINER_PIPE_FOLDER, pipe_name))
    }
}

pub fn get_idle_container() -> Result<Container, WorkerError> {
    GLOBAL_IDLE_CONTAINER_CREATOR.get_idle_container()
}
