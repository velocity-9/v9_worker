use std::fs::File;
use std::fs::OpenOptions;
use std::os::raw::c_int;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::RawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::poll::{poll, PollFd, PollFlags};
use nix::sys::stat::Mode;
use nix::unistd::{mkfifo, read, write};
use tempfile::TempDir;

use crate::error::{WorkerError, WorkerErrorKind};

#[derive(Debug)]
pub struct NamedPipeCreator {
    dir: TempDir,
    counter: AtomicU32,
}

impl NamedPipeCreator {
    pub fn new() -> Result<Self, WorkerError> {
        Ok(Self {
            dir: TempDir::new()?,
            counter: AtomicU32::new(0),
        })
    }

    pub fn new_pipe(&self) -> Result<NamedPipe, WorkerError> {
        let pipe_num = self.counter.fetch_add(1, Ordering::SeqCst);

        let component_input_fifo_filename = format!("IN_{}", pipe_num);
        let component_input_fifo_path = self.dir.path().join(component_input_fifo_filename);

        let component_output_fifo_filename = format!("OUT_{}", pipe_num);
        let component_output_fifo_path = self.dir.path().join(component_output_fifo_filename);

        mkfifo(
            &component_input_fifo_path,
            Mode::S_IRWXU | Mode::S_IRWXG | Mode::S_IRWXO,
        )?;
        mkfifo(
            &component_output_fifo_path,
            Mode::S_IRWXU | Mode::S_IRWXG | Mode::S_IRWXO,
        )?;

        debug!(
            "Creating new pipe I = {:?}, O = {:?}",
            component_input_fifo_path, component_output_fifo_path
        );

        Ok(NamedPipe {
            component_input_fifo_path,
            component_output_fifo_path,

            component_input_fifo_file: None,
            component_output_fifo_file: None,
        })
    }
}

#[derive(Debug)]
pub struct NamedPipe {
    component_input_fifo_path: PathBuf,
    component_output_fifo_path: PathBuf,

    component_input_fifo_file: Option<File>,
    component_output_fifo_file: Option<File>,
}

const PIPE_CREATION_TIMEOUT_MS: u64 = 100;
const PIPE_IO_TIMEOUT_MS: u64 = 1000;
const PIPE_POLL_INTERVAL_MS: u64 = 2;

const BUF_SIZE: usize = 256;

impl NamedPipe {
    fn get_fds(&mut self) -> Result<(RawFd, RawFd), WorkerError> {
        let deadline = Instant::now() + Duration::from_millis(PIPE_CREATION_TIMEOUT_MS);

        while self.component_output_fifo_file.is_none() && Instant::now() < deadline {
            let c_out_res = OpenOptions::new()
                .read(true)
                .custom_flags(OFlag::O_NONBLOCK.bits())
                .open(&self.component_output_fifo_path);

            debug!("Opening component output {:?}", c_out_res);

            self.component_output_fifo_file = c_out_res.ok();

            sleep(Duration::from_millis(PIPE_POLL_INTERVAL_MS))
        }

        while self.component_input_fifo_file.is_none() && Instant::now() < deadline {
            let c_in_res = OpenOptions::new()
                .write(true)
                .custom_flags(OFlag::O_NONBLOCK.bits())
                .open(&self.component_input_fifo_path);

            debug!("Opening component input {:?}", c_in_res);

            self.component_input_fifo_file = c_in_res.ok();

            sleep(Duration::from_millis(PIPE_POLL_INTERVAL_MS))
        }

        if let (Some(c_in), Some(c_out)) =
            (&self.component_input_fifo_file, &self.component_output_fifo_file)
        {
            Ok((c_in.as_raw_fd(), c_out.as_raw_fd()))
        } else {
            Err(WorkerErrorKind::OperationTimedOut.into())
        }
    }

    pub fn write(&mut self, v: &[u8]) -> Result<(), WorkerError> {
        let (c_in_fd, _) = self.get_fds()?;

        let deadline = Instant::now() + Duration::from_millis(PIPE_IO_TIMEOUT_MS);

        assert_eq!(v.last(), Some(&b'\n'));

        let mut write_idx = 0;
        while write_idx < v.len() && Instant::now() < deadline {
            // Wait until ready
            let poll_flags = PollFlags::POLLOUT;
            poll(
                &mut [PollFd::new(c_in_fd, poll_flags)],
                (deadline - Instant::now()).as_millis() as c_int,
            )?;

            // Then write the bytes
            let written_bytes = write(c_in_fd, &v[write_idx..])?;
            write_idx += written_bytes;
        }

        // If we didn't write everything, we timed out
        if write_idx < v.len() {
            return Err(WorkerErrorKind::OperationTimedOut.into());
        }

        Ok(())
    }

    pub fn component_input_file(&self) -> &PathBuf {
        &self.component_input_fifo_path
    }

    pub fn read(&mut self) -> Result<Vec<u8>, WorkerError> {
        let (_, c_out_fd) = self.get_fds()?;

        let deadline = Instant::now() + Duration::from_millis(PIPE_IO_TIMEOUT_MS);

        // Then read the bytes
        let mut read_buf = vec![0; BUF_SIZE];
        let mut result = Vec::with_capacity(BUF_SIZE);
        loop {
            // Wait for data to be available
            debug!("Polling {:?}", self.component_output_fifo_path);
            let poll_flags = PollFlags::POLLIN;
            poll(
                &mut [PollFd::new(c_out_fd, poll_flags)],
                (deadline - Instant::now()).as_millis() as c_int,
            )?;

            // If we've timed out, then just return an error
            if Instant::now() > deadline {
                return Err(WorkerErrorKind::OperationTimedOut.into());
            }

            // Otherwise read n bytes
            let n = match read(c_out_fd, &mut read_buf) {
                Ok(n) => n,
                Err(e) => {
                    if e.as_errno() == Some(Errno::EAGAIN) {
                        debug!("Trying again");
                        sleep(Duration::from_millis(PIPE_POLL_INTERVAL_MS));
                        continue;
                    } else {
                        return Err(e.into());
                    }
                }
            };

            // Reading 0 bytes indicates unix doesn't think there is more to read
            if n == 0 {
                return Err(WorkerErrorKind::SubprocessDisconnected.into());
            }

            for &v in &read_buf[0..n] {
                result.push(v);
                if v == b'\n' {
                    return Ok(result);
                }
            }
        }
    }

    pub fn component_output_file(&self) -> &PathBuf {
        &self.component_output_fifo_path
    }
}
