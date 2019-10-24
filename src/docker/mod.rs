pub mod idle_container_creator;

use std::ffi::OsStr;
use std::fmt::Debug;

use regex::Regex;
use subprocess::{Exec, ExitStatus, Popen, PopenConfig, Redirection};

use crate::error::{WorkerError, WorkerErrorKind};

pub fn call_docker_sync<S: AsRef<OsStr> + Debug>(
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

pub fn call_docker_async(docker_args: &[&str]) -> Result<Popen, WorkerError> {
    debug!("Calling (async) docker {:?}", docker_args);

    let mut argv = Vec::with_capacity(docker_args.len() + 1);
    argv.push("docker");
    argv.extend_from_slice(docker_args);

    let mut docker_subprocess = Popen::create(&argv, PopenConfig::default())?;
    docker_subprocess.detach();
    trace!("Created and detachted async docker process");

    Ok(docker_subprocess)
}

pub fn exec_in_container_sync(
    container: &str,
    command: &[&str],
) -> Result<(ExitStatus, String, String), WorkerError> {
    let mut docker_args = Vec::new();
    docker_args.push("exec");
    docker_args.push(container);
    docker_args.extend_from_slice(command);
    call_docker_sync(&docker_args)
}

pub fn exec_in_container_async(container: &str, command: &[&str]) -> Result<Popen, WorkerError> {
    let mut docker_args = Vec::new();
    docker_args.push("exec");
    docker_args.push(container);
    docker_args.extend_from_slice(command);
    call_docker_async(&docker_args)
}

pub fn load_docker_image(archive_file: &str) -> Result<String, WorkerError> {
    // we are calling docker load, with quiet mode enabled to suppress excess output
    let (load_exit_status, load_stdout, load_stderr) =
        call_docker_sync(&["load", "-q", "--input", archive_file])?;

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
    Ok(tag.to_string())
}
