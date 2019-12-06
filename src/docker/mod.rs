pub mod idle_container_creator;

use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs::remove_file;
use std::path::Path;

use rand;
use regex::Regex;
use subprocess::{Exec, ExitStatus, Popen, PopenConfig, Redirection};

use crate::error::{WorkerError, WorkerErrorKind};
use crate::fs_utils::canonicalize;
use crate::named_pipe::NamedPipe;

fn call_docker_sync<S: AsRef<OsStr> + Debug>(
    argv: &[S],
) -> Result<(ExitStatus, String, String), WorkerError> {
    debug!("Calling (sync) docker {:?}", argv);
    let docker_res = Exec::cmd("docker")
        .args(argv)
        .stdout(Redirection::Pipe)
        .stderr(Redirection::Pipe)
        .capture()?;
    let exit_status = docker_res.exit_status;
    let stdout = String::from_utf8(docker_res.stdout)?;
    let stderr = String::from_utf8(docker_res.stderr)?;

    if !exit_status.success() {
        return Err(WorkerErrorKind::Docker(exit_status, stdout, stderr).into());
    }
    Ok((exit_status, stdout, stderr))
}

fn call_docker_async(docker_args: &[&str]) -> Result<Popen, WorkerError> {
    debug!("Calling (async) docker {:?}", docker_args);

    let mut argv = Vec::with_capacity(docker_args.len() + 1);
    argv.push("docker");
    argv.extend_from_slice(docker_args);

    let mut docker_subprocess = Popen::create(&argv, PopenConfig::default())?;
    docker_subprocess.detach();
    trace!("Created and detachted async docker process");

    Ok(docker_subprocess)
}

#[derive(Debug)]
pub struct V9Container {
    named_pipe: NamedPipe,

    docker_container_name: String,
    docker_run_process: Popen,
}

fn container_name(image: &str) -> String {
    let id: u64 = rand::random();
    let res = format!("v9_{}_{}", image, id);

    // Remove the invalid colon in the middle of the image name
    res.replace(":", "_")
}

impl V9Container {
    pub fn start(pipe: NamedPipe, image: &str, image_arguments: &[&str]) -> Result<Self, WorkerError> {
        let name = container_name(image);

        let c_in = canonicalize(pipe.component_input_file())?;
        let c_out = canonicalize(pipe.component_output_file())?;

        // Call docker run, mounting the input and output pipes
        let input_mount = format!("{}:{}", c_in, c_in);
        let output_mount = format!("{}:{}", c_out, c_out);
        let mut docker_args = vec![
            "run",
            "--name",
            &name,
            "-v",
            &input_mount,
            "-v",
            &output_mount,
            image,
        ];

        docker_args.extend_from_slice(image_arguments);

        let docker_subprocess = call_docker_async(&docker_args)?;

        Ok(Self {
            named_pipe: pipe,
            docker_container_name: name,
            docker_run_process: docker_subprocess,
        })
    }

    pub fn pipe(&mut self) -> &mut NamedPipe {
        &mut self.named_pipe
    }

    pub fn process(&mut self) -> &mut Popen {
        &mut self.docker_run_process
    }

    pub fn exec_sync(&self, command: &[&str]) -> Result<(ExitStatus, String, String), WorkerError> {
        let mut docker_args = vec!["exec", &self.docker_container_name];
        docker_args.extend_from_slice(command);
        call_docker_sync(&docker_args)
    }

    pub fn exec_async(&self, command: &[&str]) -> Result<Popen, WorkerError> {
        let mut docker_args = vec!["exec", &self.docker_container_name];
        docker_args.extend_from_slice(command);
        call_docker_async(&docker_args)
    }

    pub fn copy_directory_in(&self, source_dir: &str, target_dir: &str) -> Result<(), WorkerError> {
        // Paths that end with `/.` tell docker to copy contents
        let source = format!("{}/.", source_dir);

        call_docker_sync(&[
            "cp",
            &source,
            &format!("{}:{}", self.docker_container_name, target_dir),
        ])?;

        Ok(())
    }
}

impl Drop for V9Container {
    fn drop(&mut self) {
        if let Err(e) = self.docker_run_process.terminate() {
            self.docker_run_process.detach();

            error!("Could not terminate docker process: {}", e)
        }
    }
}

pub fn load_docker_image(archive_file: &str) -> Result<String, WorkerError> {
    // we are calling docker load, with quiet mode enabled to suppress excess output
    let (load_exit_status, load_stdout, load_stderr) =
        call_docker_sync(&["load", "-q", "-i", archive_file])?;

    let regex = Regex::new("Loaded image( ID)?: (?P<tag>.*)\n")?;
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

    match remove_file(Path::new(archive_file)) {
        Ok(_) => debug!("Deleted tar file {}", archive_file),
        Err(e) => error!("Failed to delete tar file after loading image: {}", e),
    }

    Ok(tag.to_string())
}
