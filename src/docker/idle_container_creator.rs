use std::sync::mpsc::{sync_channel, Receiver};
use std::thread;
use std::time::Duration;

use lazy_static::lazy_static;
use parking_lot::Mutex;

use crate::docker::V9Container;
use crate::error::WorkerError;
use crate::named_pipe::NamedPipe;

// We guarantee that the new idle containers have this code folder available
pub const CODE_FOLDER: &str = "/home/sl";

// NOTE: the number of idle containers on the system is CONTAINER_CACHE_CHANNEL_SIZE + CACHE_POPULATOR_COUNT
const CONTAINER_CACHE_CHANNEL_SIZE: usize = 3;
const CACHE_POPULATOR_COUNT: usize = 2;

const CONTAINER_IMAGE_TAG: &str = "python:3.7-alpine";
// 1000000000 seconds ~= 30 years
const SLEEP_TIME: &str = "1000000000";

fn sync_create_container() -> Result<V9Container, WorkerError> {
    let pipe = NamedPipe::new()?;
    let container = V9Container::start(pipe, CONTAINER_IMAGE_TAG, &["sleep", SLEEP_TIME])?;

    // Unfortunately we can't know when the container is ready, so we blindly sleep for a second
    // Luckily this is usually done in an async context, so it's okay to sleep
    thread::sleep(Duration::from_secs(1));

    container.exec_sync(&["mkdir", "-p", CODE_FOLDER])?;

    Ok(container)
}

pub struct IdleContainerCreator {
    cache_channel_receiver: Mutex<Receiver<V9Container>>,
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

    fn get_idle_container(&self) -> Result<V9Container, WorkerError> {
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

pub fn get_idle_container() -> Result<V9Container, WorkerError> {
    GLOBAL_IDLE_CONTAINER_CREATOR.get_idle_container()
}
