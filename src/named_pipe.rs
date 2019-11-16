use std::convert::TryInto;
use std::fs::File;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::io::RawFd;
use std::path::{Path, PathBuf};
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
pub struct NamedPipe {
    root_folder: TempDir,

    component_input_fifo_path: PathBuf,
    component_output_fifo_path: PathBuf,

    component_input_fifo_file: Option<File>,
    component_output_fifo_file: Option<File>,
}

// TODO: Justify these values more

// This is basically our limit on startup time
const PIPE_CREATION_TIMEOUT_MS: u64 = 10000;
// This is basically our limit on individual call time
const PIPE_IO_TIMEOUT_MS: u64 = 10000;
// This is a knob for our cpu usage during calls
const PIPE_POLL_INTERVAL_MS: u64 = 3;

// How much we should read from the component at the time
const BUF_SIZE: usize = 512;

impl NamedPipe {
    pub fn new() -> Result<Self, WorkerError> {
        let dir = TempDir::new()?;
        Ok(Self::in_dir(dir)?)
    }

    pub fn in_dir(dir: TempDir) -> Result<Self, WorkerError> {
        let component_input_fifo_path = dir.path().join("IN");
        let component_output_fifo_path = dir.path().join("OUT");

        // These fifos are created with 777 permissions
        mkfifo(
            &component_input_fifo_path,
            Mode::S_IRWXU | Mode::S_IRWXG | Mode::S_IRWXO,
        )?;
        mkfifo(
            &component_output_fifo_path,
            Mode::S_IRWXU | Mode::S_IRWXG | Mode::S_IRWXO,
        )?;

        debug!(
            "Creating new pipes I = {:?}, O = {:?}",
            component_input_fifo_path, component_output_fifo_path
        );

        Ok(Self {
            root_folder: dir,

            component_input_fifo_path,
            component_output_fifo_path,

            component_input_fifo_file: None,
            component_output_fifo_file: None,
        })
    }

    fn get_fds(&mut self) -> Result<(RawFd, RawFd), WorkerError> {
        let deadline = Instant::now() + Duration::from_millis(PIPE_CREATION_TIMEOUT_MS);

        while self.component_output_fifo_file.is_none() && Instant::now() < deadline {
            let c_out_res = OpenOptions::new()
                .read(true)
                .custom_flags(OFlag::O_NONBLOCK.bits())
                .open(&self.component_output_fifo_path);

            trace!("Opening component output {:?}", c_out_res);

            self.component_output_fifo_file = c_out_res.ok();

            sleep(Duration::from_millis(PIPE_POLL_INTERVAL_MS))
        }

        while self.component_input_fifo_file.is_none() && Instant::now() < deadline {
            let c_in_res = OpenOptions::new()
                .write(true)
                .custom_flags(OFlag::O_NONBLOCK.bits())
                .open(&self.component_input_fifo_path);

            trace!("Opening component input {:?}", c_in_res);

            self.component_input_fifo_file = c_in_res.ok();

            sleep(Duration::from_millis(PIPE_POLL_INTERVAL_MS))
        }

        trace!("Finished trying to open component pipes");

        if let (Some(c_in), Some(c_out)) =
            (&self.component_input_fifo_file, &self.component_output_fifo_file)
        {
            Ok((c_in.as_raw_fd(), c_out.as_raw_fd()))
        } else {
            Err(WorkerErrorKind::OperationTimedOut("fifo pipe opening").into())
        }
    }

    // Precondition: No newlines in the input string
    pub fn write(&mut self, v: &[u8]) -> Result<(), WorkerError> {
        // Passing in a newline violates the contract of this method
        if v.contains(&b'\n') {
            return Err(WorkerErrorKind::InvalidSerialization("contains newline", v.to_vec()).into());
        }

        // Push a newline at the end to terminate the input
        let mut v = Vec::from(v);
        v.push(b'\n');

        let (c_in_fd, _) = self.get_fds()?;

        let deadline = Instant::now() + Duration::from_millis(PIPE_IO_TIMEOUT_MS);

        let mut write_idx = 0;
        while write_idx < v.len() && Instant::now() < deadline {
            trace!("Polling {:?}", self.component_input_fifo_path);
            // Wait until ready
            let poll_flags = PollFlags::POLLOUT;
            poll(
                &mut [PollFd::new(c_in_fd, poll_flags)],
                (deadline - Instant::now()).as_millis().try_into()?,
            )?;

            // Then write the bytes
            let written_bytes = write(c_in_fd, &v[write_idx..])?;
            write_idx += written_bytes;
        }

        // If we didn't write everything, we timed out
        if write_idx < v.len() {
            return Err(WorkerErrorKind::OperationTimedOut("pipe writing").into());
        }

        Ok(())
    }

    pub fn component_input_file(&self) -> &Path {
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
            trace!("Polling {:?}", self.component_output_fifo_path);
            let poll_flags = PollFlags::POLLIN;
            poll(
                &mut [PollFd::new(c_out_fd, poll_flags)],
                (deadline - Instant::now()).as_millis().try_into()?,
            )?;

            // If we've timed out, then just return an error
            if Instant::now() > deadline {
                return Err(WorkerErrorKind::OperationTimedOut("pipe reading").into());
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
                return Err(WorkerErrorKind::PipeDisconnected.into());
            }

            for &v in &read_buf[0..n] {
                result.push(v);
                if v == b'\n' {
                    return Ok(result);
                }
            }
        }
    }

    pub fn component_output_file(&self) -> &Path {
        &self.component_output_fifo_path
    }

    pub fn query(&mut self, req: &str) -> Result<String, WorkerError> {
        self.write(req.as_bytes())?;

        let read_bytes = self.read()?;
        Ok(String::from_utf8(read_bytes)?)
    }
}
