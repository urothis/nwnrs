#[cfg(any(unix, feature = "tooling"))]
use std::io::Read as _;
use std::{
    fs,
    io::{BufRead as _, Seek as _, Write as _},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitCode, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use nwnrs_runtime::{
    BinaryIdentity, ENV_ENABLED, ENV_REQUIRED, ENV_SUPERVISED, ENV_TARGET_DIR, ENV_TARGET_PACK,
    OperatingSystem, Platform, resolve_target_pack,
};
use tracing::{info, instrument};

use crate::args::{ColorMode, RunCmd};

const LINUX_PRELOAD: &str = "LD_PRELOAD";
const MACOS_PRELOAD: &str = "DYLD_INSERT_LIBRARIES";
const RUNTIME_COLOR: &str = "NWNRS_COLOR";
#[cfg(any(unix, windows))]
const DUPLICATE_SHUTDOWN_SIGNAL_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);
#[cfg(feature = "tooling")]
const DEFAULT_DOCKER_HOME: &str = "nwserver-home";
#[cfg(feature = "tooling")]
const DEFAULT_DOCKER_IMAGE: &str = "nwserver:local";
#[cfg(feature = "tooling")]
const DEFAULT_DOCKER_PUBLISH: &str = "5121:5121/udp";
#[cfg(feature = "tooling")]
const DOCKER_DAEMON_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(any(unix, windows))]
enum ServerInput {
    Data(Vec<u8>),
    End,
    Shutdown,
}

#[cfg(any(unix, windows))]
#[derive(Clone, Copy)]
enum ServerLogKind {
    Output,
    Error,
}

#[cfg(any(unix, windows))]
struct LogFollower {
    stop:  Arc<AtomicBool>,
    relay: Option<std::thread::JoinHandle<()>>,
}

struct LaunchPlan {
    server:            PathBuf,
    runtime:           PathBuf,
    target_pack:       PathBuf,
    working_directory: PathBuf,
    server_args:       Vec<String>,
    platform:          Platform,
    log_paths:         Option<[PathBuf; 2]>,
    color:             ColorMode,
    #[cfg(target_os = "linux")]
    container:         Option<crate::container::ContainerState>,
}

#[cfg(feature = "tooling")]
struct DockerLaunchPlan {
    image: String,
    args:  Vec<String>,
}

#[instrument(level = "info", skip_all)]
pub(crate) fn run_server(command: RunCmd) -> Result<ExitCode, String> {
    #[cfg(feature = "tooling")]
    if command.docker {
        if command.container {
            return Err("--docker and --container cannot be used together".to_string());
        }
        return DockerLaunchPlan::prepare(command)?.execute();
    }
    let plan = LaunchPlan::prepare(command)?;
    execute(plan)
}

impl LaunchPlan {
    fn prepare(command: RunCmd) -> Result<Self, String> {
        #[cfg(feature = "tooling")]
        validate_native_options(&command)?;
        #[cfg(target_os = "linux")]
        let (command, container) = prepare_container_command(command)?;
        #[cfg(not(target_os = "linux"))]
        let command = validate_non_linux_container_mode(command)?;
        let runtime_path = command.runtime.ok_or_else(|| {
            "native mode requires --runtime; use --docker to start the container image".to_string()
        })?;
        let targets = command.targets.ok_or_else(|| {
            "native mode requires --targets; use --docker to start the container image".to_string()
        })?;
        let (server_path, server_args) = command
            .arguments
            .split_first()
            .ok_or_else(|| "native mode requires the nwserver executable after --".to_string())?;
        let host = Platform::host().map_err(|error| error.to_string())?;
        let server = BinaryIdentity::read(server_path).map_err(|error| error.to_string())?;
        if server.platform != host {
            return Err(format!(
                "server platform {} does not match launcher host {host}",
                server.platform
            ));
        }

        let runtime = BinaryIdentity::read(runtime_path).map_err(|error| error.to_string())?;
        if runtime.platform != server.platform {
            return Err(format!(
                "runtime platform {} does not match server platform {}",
                runtime.platform, server.platform
            ));
        }

        let target = resolve_target_pack(&targets, &server).map_err(|error| error.to_string())?;
        let working_directory = match command.working_directory {
            Some(path) => canonical_directory(path)?,
            None => server
                .path
                .parent()
                .map(PathBuf::from)
                .ok_or_else(|| "server binary does not have a parent directory".to_string())?,
        };
        let log_paths = if command.no_tail_logs {
            None
        } else {
            Some(server_log_paths(server_args, &working_directory)?)
        };

        Ok(Self {
            server: server.path,
            runtime: runtime.path,
            target_pack: target.path,
            working_directory,
            server_args: server_args.to_vec(),
            platform: server.platform,
            log_paths,
            color: command.color,
            #[cfg(target_os = "linux")]
            container,
        })
    }

    fn command(&self) -> ProcessCommand {
        let mut command = ProcessCommand::new(&self.server);
        command
            .args(&self.server_args)
            .current_dir(&self.working_directory)
            .env_remove(LINUX_PRELOAD)
            .env_remove(MACOS_PRELOAD)
            .env_remove(ENV_TARGET_DIR)
            .env(ENV_ENABLED, "1")
            .env(ENV_REQUIRED, "1")
            .env(ENV_SUPERVISED, "1")
            .env(ENV_TARGET_PACK, &self.target_pack)
            .env(RUNTIME_COLOR, self.color.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match self.platform.os {
            OperatingSystem::Macos => {
                command.env(MACOS_PRELOAD, &self.runtime);
            }
            OperatingSystem::Linux => {
                command.env(LINUX_PRELOAD, &self.runtime);
            }
            OperatingSystem::Windows => {}
        }
        isolate_process_group(&mut command);
        command
    }

    #[cfg(unix)]
    fn backup_container_configuration(&self) {
        #[cfg(target_os = "linux")]
        if let Some(container) = &self.container {
            container.backup_configuration();
        }
    }

    #[cfg(unix)]
    fn finalize_container(&mut self) {
        #[cfg(target_os = "linux")]
        if let Some(container) = &mut self.container {
            container.finalize();
        }
    }

    #[cfg(any(unix, windows))]
    fn log_follower_paths(&self) -> Vec<(ServerLogKind, PathBuf)> {
        let Some(paths) = self.log_paths.as_ref() else {
            return Vec::new();
        };
        [ServerLogKind::Output, ServerLogKind::Error]
            .into_iter()
            .zip(paths)
            .map(|(kind, path)| (kind, path.clone()))
            .collect()
    }
}

#[cfg(target_os = "linux")]
fn prepare_container_command(
    mut command: RunCmd,
) -> Result<(RunCmd, Option<crate::container::ContainerState>), String> {
    if !command.container {
        return Ok((command, None));
    }
    if command.runtime.is_some() || command.targets.is_some() || command.working_directory.is_some()
    {
        return Err(
            "--runtime, --targets, and --working-directory cannot override container mode"
                .to_string(),
        );
    }
    let launch = crate::container::ContainerLaunch::prepare(&command.arguments)?;
    command.runtime = Some(launch.runtime);
    command.targets = Some(launch.targets);
    command.working_directory = Some(launch.working_directory);
    command.arguments = std::iter::once(launch.server.to_string_lossy().into_owned())
        .chain(launch.server_args)
        .collect();
    command.no_tail_logs |= !launch.tail_logs;
    Ok((command, Some(launch.state)))
}

#[cfg(not(target_os = "linux"))]
fn validate_non_linux_container_mode(command: RunCmd) -> Result<RunCmd, String> {
    if command.container {
        Err("--container is supported only by the Linux image".to_string())
    } else {
        Ok(command)
    }
}

#[cfg(feature = "tooling")]
fn validate_native_options(command: &RunCmd) -> Result<(), String> {
    if command.docker_image != DEFAULT_DOCKER_IMAGE
        || command.docker_name.is_some()
        || command.docker_home != DEFAULT_DOCKER_HOME
        || !command.docker_publish.is_empty()
        || !command.docker_arg.is_empty()
    {
        return Err("Docker options require --docker".to_string());
    }
    Ok(())
}

#[cfg(feature = "tooling")]
impl DockerLaunchPlan {
    fn prepare(command: RunCmd) -> Result<Self, String> {
        use std::io::IsTerminal as _;

        if command.runtime.is_some()
            || command.targets.is_some()
            || command.working_directory.is_some()
        {
            return Err(
                "--runtime, --targets, and --working-directory are native-mode options".to_string(),
            );
        }
        if command.docker_image.is_empty() {
            return Err("--docker-image cannot be empty".to_string());
        }
        if command.docker_home.is_empty() {
            return Err("--docker-home cannot be empty".to_string());
        }
        if command
            .docker_name
            .as_ref()
            .is_some_and(|name| name.is_empty())
        {
            return Err("--docker-name cannot be empty".to_string());
        }
        if command.docker_publish.iter().any(String::is_empty) {
            return Err("--docker-publish cannot be empty".to_string());
        }
        if command
            .docker_arg
            .iter()
            .any(|argument| argument.is_empty() || argument.starts_with('-'))
        {
            return Err(
                "--docker-arg must be a non-empty Docker long option without leading dashes"
                    .to_string(),
            );
        }

        let interactive = std::io::stdin().is_terminal();
        let tty = interactive && std::io::stdout().is_terminal() && std::io::stderr().is_terminal();
        Ok(Self::from_command(command, interactive, tty))
    }

    fn from_command(command: RunCmd, interactive: bool, tty: bool) -> Self {
        let has_pull_policy = command
            .docker_arg
            .iter()
            .any(|argument| argument.starts_with("pull="));
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--read-only".to_string(),
            "--cap-drop=ALL".to_string(),
            "--security-opt=no-new-privileges=true".to_string(),
            "--tmpfs=/nwn/run:uid=1000,gid=0,mode=0770".to_string(),
            "--tmpfs=/tmp:uid=1000,gid=0,mode=1777".to_string(),
        ];
        if !has_pull_policy {
            args.push("--pull=never".to_string());
        }
        if interactive {
            args.push("--interactive".to_string());
        }
        if tty {
            args.push("--tty".to_string());
        }
        if let Some(name) = command.docker_name {
            args.extend(["--name".to_string(), name]);
        }
        let publishes = if command.docker_publish.is_empty() {
            vec![DEFAULT_DOCKER_PUBLISH.to_string()]
        } else {
            command.docker_publish
        };
        for publish in publishes {
            args.extend(["--publish".to_string(), publish]);
        }
        args.extend([
            "--volume".to_string(),
            format!("{}:/nwn/home", command.docker_home),
            "--env".to_string(),
            format!("NWNRS_COLOR={}", command.color.as_str()),
        ]);
        if command.no_tail_logs {
            args.extend(["--env".to_string(), "NWN_TAIL_LOGS=n".to_string()]);
        }
        args.extend(
            command
                .docker_arg
                .into_iter()
                .map(|argument| format!("--{argument}")),
        );
        let image = command.docker_image;
        args.push(image.clone());
        args.extend(command.arguments);
        Self {
            image,
            args,
        }
    }

    #[cfg(unix)]
    fn execute(self) -> Result<ExitCode, String> {
        use std::os::unix::process::CommandExt as _;

        ensure_docker_daemon()?;
        info!(
            target: "nwnrs::launcher",
            image = %self.image,
            "starting NWServer container"
        );
        let error = ProcessCommand::new("docker").args(&self.args).exec();
        Err(format!("failed to execute Docker CLI: {error}"))
    }

    #[cfg(not(unix))]
    fn execute(self) -> Result<ExitCode, String> {
        ensure_docker_daemon()?;
        info!(
            target: "nwnrs::launcher",
            image = %self.image,
            "starting NWServer container"
        );
        let status = ProcessCommand::new("docker")
            .args(&self.args)
            .status()
            .map_err(|error| format!("failed to execute Docker CLI: {error}"))?;
        let code = status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1);
        Ok(ExitCode::from(code))
    }
}

#[cfg(feature = "tooling")]
fn ensure_docker_daemon() -> Result<(), String> {
    let mut child = ProcessCommand::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to execute Docker CLI: {error}"))?;
    let deadline = std::time::Instant::now() + DOCKER_DAEMON_TIMEOUT;

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut detail = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut detail);
                }
                let detail = detail.trim();
                return if detail.is_empty() {
                    Err(format!("Docker daemon check failed with {status}"))
                } else {
                    Err(format!("Docker daemon check failed: {detail}"))
                };
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "Docker daemon did not respond within {} seconds; verify the active Docker \
                     context or restart Docker Desktop",
                    DOCKER_DAEMON_TIMEOUT.as_secs()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("failed while checking the Docker daemon: {error}"));
            }
        }
    }
}

#[cfg(unix)]
fn isolate_process_group(command: &mut ProcessCommand) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut ProcessCommand) {}

fn server_log_paths(
    server_args: &[String],
    working_directory: &Path,
) -> Result<[PathBuf; 2], String> {
    let mut forwarded_user_directory = None;
    let mut arguments = server_args.iter();
    while let Some(argument) = arguments.next() {
        if argument.eq_ignore_ascii_case("-userdirectory") {
            let value = arguments.next().ok_or_else(|| {
                "forwarded -userdirectory option is missing its directory".to_string()
            })?;
            forwarded_user_directory = Some(PathBuf::from(value));
            continue;
        }

        if let Some((option, value)) = argument.split_once('=')
            && option.eq_ignore_ascii_case("-userdirectory")
        {
            if value.is_empty() {
                return Err("forwarded -userdirectory option has an empty directory".to_string());
            }
            forwarded_user_directory = Some(PathBuf::from(value));
        }
    }

    let user_directory = match forwarded_user_directory {
        Some(path) if path.is_absolute() => path,
        Some(path) => working_directory.join(path),
        None => default_user_directory()?,
    };
    let logs = user_directory.join("logs.0");
    Ok([
        logs.join("nwserverLog1.txt"),
        logs.join("nwserverError1.txt"),
    ])
}

fn default_user_directory() -> Result<PathBuf, String> {
    for variable in ["NWN_HOME", "NWN_USER_DIRECTORY"] {
        if let Some(path) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
            let path = PathBuf::from(path);
            return path.is_dir().then_some(path).ok_or_else(|| {
                format!(
                    "{variable} does not name an existing directory; pass -userdirectory to \
                     nwserver or --no-tail-logs to nwnrs run"
                )
            });
        }
    }

    let home_variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(home_variable)
        .map(PathBuf::from)
        .ok_or_else(|| {
            format!(
                "{home_variable} is unavailable; pass -userdirectory to nwserver, set NWN_HOME, \
                 or pass --no-tail-logs to nwnrs run"
            )
        })?;
    let candidates = if cfg!(target_os = "macos") {
        vec![
            home.join("Documents").join("Neverwinter Nights"),
            home.join("Library")
                .join("Application Support")
                .join("Neverwinter Nights"),
        ]
    } else if cfg!(windows) {
        vec![home.join("Documents").join("Neverwinter Nights")]
    } else {
        vec![home.join(".local").join("share").join("Neverwinter Nights")]
    };
    candidates
        .into_iter()
        .find(|path| path.is_dir())
        .ok_or_else(|| {
            "could not locate the NWN user directory; pass -userdirectory to nwserver, set \
             NWN_HOME, or pass --no-tail-logs to nwnrs run"
                .to_string()
        })
}

fn canonical_directory(path: PathBuf) -> Result<PathBuf, String> {
    let path = fs::canonicalize(&path).map_err(|error| {
        format!(
            "failed to resolve working directory {}: {error}",
            path.display()
        )
    })?;
    if !path.is_dir() {
        return Err(format!(
            "working directory is not a directory: {}",
            path.display()
        ));
    }
    Ok(path)
}

#[cfg(unix)]
fn execute(plan: LaunchPlan) -> Result<ExitCode, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to initialize launcher supervision: {error}"))?;
    runtime.block_on(supervise(plan))
}

#[cfg(windows)]
fn execute(plan: LaunchPlan) -> Result<ExitCode, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to initialize launcher supervision: {error}"))?;
    runtime.block_on(supervise_windows(plan))
}

#[cfg(not(any(unix, windows)))]
fn execute(_plan: LaunchPlan) -> Result<ExitCode, String> {
    Err("the native runtime launcher does not support this operating system".to_string())
}

#[cfg(windows)]
async fn supervise_windows(plan: LaunchPlan) -> Result<ExitCode, String> {
    use nwnrs_runtime_sys::spawn_injected_windows;
    use tokio::{signal::windows, time};

    let mut interrupt =
        windows::ctrl_c().map_err(|error| format!("failed to listen for CTRL_C: {error}"))?;
    let mut break_signal = windows::ctrl_break()
        .map_err(|error| format!("failed to listen for CTRL_BREAK: {error}"))?;
    info!(
        target: "nwnrs::launcher",
        platform = %plan.platform,
        "starting NWServer"
    );
    tracing::debug!(
        target: "nwnrs::launcher",
        server = %plan.server.display(),
        runtime = %plan.runtime.display(),
        target_pack = %plan.target_pack.display(),
        working_directory = %plan.working_directory.display(),
        "resolved launch artifacts"
    );

    let mut log_followers = start_log_followers(&plan)?;
    let mut command = plan.command();
    let (mut server, control) = match spawn_injected_windows(&mut command, &plan.runtime) {
        Ok(process) => process,
        Err(error) => {
            stop_log_followers(&mut log_followers);
            return Err(format!(
                "failed to start injected server {}: {error}",
                plan.server.display()
            ));
        }
    };
    let pipes = (|| {
        Ok::<_, String>((
            take_windows_pipe(&mut server, |child| child.stdin.take(), "input")?,
            take_windows_pipe(&mut server, |child| child.stdout.take(), "output")?,
            take_windows_pipe(&mut server, |child| child.stderr.take(), "error")?,
        ))
    })();
    let (server_input, server_output, server_error) = match pipes {
        Ok(pipes) => pipes,
        Err(error) => {
            stop_log_followers(&mut log_followers);
            return Err(error);
        }
    };
    info!(
        target: "nwnrs::launcher",
        process_id = server.id(),
        "NWServer process started and runtime initialized"
    );

    let (input_sender, input_receiver) = std::sync::mpsc::channel();
    let (shutdown_sender, mut shutdown_receiver) = tokio::sync::mpsc::unbounded_channel();
    start_windows_terminal_input_relay(input_sender.clone(), shutdown_sender);
    let input_writer = start_server_input_writer(server_input, input_receiver);
    let console_relay = start_server_console_relay(server_output);
    let error_relay = start_server_error_relay(server_error);
    let mut shutdown_requested_at = None;
    let mut wait_interval = time::interval(std::time::Duration::from_millis(50));
    wait_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    let status = loop {
        tokio::select! {
            _ = wait_interval.tick() => {
                match server.try_wait() {
                    Ok(Some(status)) => break Ok(status),
                    Ok(None) => {}
                    Err(error) => break Err(format!(
                        "failed while waiting for server {}: {error}",
                        plan.server.display()
                    )),
                }
            }
            received = interrupt.recv() => {
                if received.is_some() {
                    request_windows_shutdown(
                        &mut server,
                        control,
                        &input_sender,
                        &mut shutdown_requested_at,
                    );
                }
            }
            received = break_signal.recv() => {
                if received.is_some() {
                    request_windows_shutdown(
                        &mut server,
                        control,
                        &input_sender,
                        &mut shutdown_requested_at,
                    );
                }
            }
            request = shutdown_receiver.recv() => {
                if request.is_some() {
                    request_windows_shutdown(
                        &mut server,
                        control,
                        &input_sender,
                        &mut shutdown_requested_at,
                    );
                }
            }
        }
    };

    let _ = input_sender.send(ServerInput::End);
    let _ = input_writer.join();
    let _ = console_relay.join();
    let _ = error_relay.join();
    stop_log_followers(&mut log_followers);
    match status {
        Ok(status) if status.success() => {
            info!(target: "nwnrs::launcher", "NWServer exited successfully");
            Ok(windows_exit_status_code(status))
        }
        Ok(status) => {
            tracing::warn!(target: "nwnrs::launcher", %status, "NWServer exited unsuccessfully");
            Ok(windows_exit_status_code(status))
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn take_windows_pipe<T>(
    server: &mut std::process::Child,
    take: impl FnOnce(&mut std::process::Child) -> Option<T>,
    name: &str,
) -> Result<T, String> {
    if let Some(pipe) = take(server) {
        return Ok(pipe);
    }
    let _ = server.kill();
    let _ = server.wait();
    Err(format!(
        "failed to open the supervised server standard {name} pipe"
    ))
}

#[cfg(windows)]
fn request_windows_shutdown(
    server: &mut std::process::Child,
    control: nwnrs_runtime_sys::WindowsProcessControl,
    input_sender: &std::sync::mpsc::Sender<ServerInput>,
    shutdown_requested_at: &mut Option<std::time::Instant>,
) {
    if let Some(requested_at) = *shutdown_requested_at {
        if requested_at.elapsed() < DUPLICATE_SHUTDOWN_SIGNAL_WINDOW {
            tracing::debug!(target: "nwnrs::launcher", "ignoring duplicate shutdown signal");
            return;
        }
        tracing::warn!(target: "nwnrs::launcher", "forcing NWServer to terminate after repeated shutdown request");
        if let Err(error) = server.kill()
            && error.kind() != std::io::ErrorKind::InvalidInput
        {
            tracing::warn!(%error, "failed to terminate NWServer");
        }
        return;
    }
    *shutdown_requested_at = Some(std::time::Instant::now());
    info!(target: "nwnrs::launcher", "requesting graceful NWServer shutdown");
    if let Err(error) = control.request_graceful_shutdown() {
        tracing::warn!(%error, "WM_QUIT failed; falling back to the native quit command");
        if input_sender.send(ServerInput::Shutdown).is_err() {
            tracing::warn!(target: "nwnrs::launcher", "server input relay is unavailable");
        }
    }
}

#[cfg(unix)]
async fn supervise(mut plan: LaunchPlan) -> Result<ExitCode, String> {
    use tokio::{signal::unix, time};

    let mut terminate = unix::signal(unix::SignalKind::terminate())
        .map_err(|error| format!("failed to listen for TERM: {error}"))?;
    let mut interrupt = unix::signal(unix::SignalKind::interrupt())
        .map_err(|error| format!("failed to listen for INT: {error}"))?;
    let mut hangup = unix::signal(unix::SignalKind::hangup())
        .map_err(|error| format!("failed to listen for HUP: {error}"))?;

    info!(
        target: "nwnrs::launcher",
        platform = %plan.platform,
        "starting NWServer"
    );
    tracing::debug!(
        target: "nwnrs::launcher",
        server = %plan.server.display(),
        runtime = %plan.runtime.display(),
        target_pack = %plan.target_pack.display(),
        working_directory = %plan.working_directory.display(),
        "resolved launch artifacts"
    );
    let mut log_followers = start_log_followers(&plan)?;
    if let Some(paths) = plan.log_paths.as_ref() {
        let [output_path, error_path] = paths;
        info!(
            target: "nwnrs::launcher",
            "following new server log output"
        );
        tracing::debug!(
            target: "nwnrs::launcher",
            output = %output_path.display(),
            error = %error_path.display(),
            "resolved server log paths"
        );
    }

    let mut command = plan.command();
    let mut server = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            stop_log_followers(&mut log_followers);
            return Err(format!(
                "failed to start server {}: {error}",
                plan.server.display()
            ));
        }
    };
    let Some(server_input) = server.stdin.take() else {
        forward_signal(&server, nix::sys::signal::Signal::SIGKILL);
        let _ = server.wait();
        stop_log_followers(&mut log_followers);
        return Err("failed to open the supervised server standard input pipe".to_string());
    };
    let Some(server_output) = server.stdout.take() else {
        forward_signal(&server, nix::sys::signal::Signal::SIGKILL);
        let _ = server.wait();
        stop_log_followers(&mut log_followers);
        return Err("failed to open the supervised server standard output pipe".to_string());
    };
    let Some(server_error) = server.stderr.take() else {
        forward_signal(&server, nix::sys::signal::Signal::SIGKILL);
        let _ = server.wait();
        stop_log_followers(&mut log_followers);
        return Err("failed to open the supervised server standard error pipe".to_string());
    };
    info!(
        target: "nwnrs::launcher",
        process_id = server.id(),
        "NWServer process started"
    );
    let (input_sender, input_receiver) = std::sync::mpsc::channel();
    start_terminal_input_relay(input_sender.clone());
    let input_writer = start_server_input_writer(server_input, input_receiver);
    let console_relay = start_server_console_relay(server_output);
    let error_relay = start_server_error_relay(server_error);
    let mut shutdown_requested_at = None;
    let mut wait_interval = time::interval(std::time::Duration::from_millis(50));
    wait_interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut configuration_backup = Box::pin(time::sleep(std::time::Duration::from_secs(10)));
    let mut configuration_backed_up = false;

    let status = loop {
        tokio::select! {
            () = &mut configuration_backup, if !configuration_backed_up => {
                plan.backup_container_configuration();
                configuration_backed_up = true;
            }
            _ = wait_interval.tick() => {
                match server.try_wait() {
                    Ok(Some(status)) => break Ok(status),
                    Ok(None) => {}
                    Err(error) => {
                        break Err(format!(
                            "failed while waiting for server {}: {error}",
                            plan.server.display()
                        ));
                    }
                }
            }
            received = terminate.recv() => {
                if received.is_some() {
                    request_server_shutdown(
                        &server,
                        &input_sender,
                        &mut shutdown_requested_at,
                    );
                }
            }
            received = interrupt.recv() => {
                if received.is_some() {
                    request_server_shutdown(
                        &server,
                        &input_sender,
                        &mut shutdown_requested_at,
                    );
                }
            }
            received = hangup.recv() => {
                if received.is_some() {
                    forward_signal(&server, nix::sys::signal::Signal::SIGHUP);
                }
            }
        }
    };

    let _ = input_sender.send(ServerInput::End);
    let _ = input_writer.join();
    let _ = console_relay.join();
    let _ = error_relay.join();
    stop_log_followers(&mut log_followers);
    plan.finalize_container();
    match status {
        Ok(status) if status.success() => {
            info!(target: "nwnrs::launcher", "NWServer exited successfully");
            Ok(exit_status_code(status))
        }
        Ok(status) => {
            tracing::warn!(
                target: "nwnrs::launcher",
                %status,
                "NWServer exited unsuccessfully"
            );
            Ok(exit_status_code(status))
        }
        Err(error) => Err(error),
    }
}

#[cfg(any(unix, windows))]
fn start_log_followers(plan: &LaunchPlan) -> Result<Vec<LogFollower>, String> {
    let mut followers = Vec::new();
    for (kind, path) in plan.log_follower_paths() {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let relay = match std::thread::Builder::new()
            .name(format!("nwnrs-log-{}", path.display()))
            .spawn(move || follow_server_log(&path, kind, &thread_stop))
        {
            Ok(relay) => relay,
            Err(error) => {
                stop_log_followers(&mut followers);
                return Err(format!(
                    "failed to start server log follower thread: {error}"
                ));
            }
        };
        followers.push(LogFollower {
            stop,
            relay: Some(relay),
        });
    }
    Ok(followers)
}

#[cfg(unix)]
fn follow_server_log(path: &Path, kind: ServerLogKind, stop: &AtomicBool) {
    use std::{
        fs::File,
        io::{BufReader, SeekFrom},
        os::unix::fs::MetadataExt as _,
        time::Duration,
    };

    let skip_existing = path.is_file();
    let mut first_open = true;
    let mut reader = None;
    let mut identity = None;
    let mut position = 0_u64;
    while !stop.load(Ordering::Relaxed) {
        if reader.is_none() {
            match File::open(path) {
                Ok(file) => {
                    let metadata = match file.metadata() {
                        Ok(metadata) => metadata,
                        Err(error) => {
                            tracing::warn!(target: "nwnrs::launcher", path = %path.display(), %error, "failed to inspect followed server log");
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                    };
                    let mut new_reader = BufReader::new(file);
                    let start = if first_open && skip_existing {
                        SeekFrom::End(0)
                    } else {
                        SeekFrom::Start(0)
                    };
                    match new_reader.seek(start) {
                        Ok(offset) => position = offset,
                        Err(error) => {
                            tracing::warn!(target: "nwnrs::launcher", path = %path.display(), %error, "failed to seek followed server log");
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                    }
                    identity = Some((metadata.dev(), metadata.ino()));
                    reader = Some(new_reader);
                    first_open = false;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(error) => {
                    tracing::warn!(target: "nwnrs::launcher", path = %path.display(), %error, "failed to open followed server log");
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }
            }
        }

        let Some(active_reader) = reader.as_mut() else {
            continue;
        };
        let mut line = String::new();
        match active_reader.read_line(&mut line) {
            Ok(0) => {
                let replaced_or_truncated = fs::metadata(path).is_ok_and(|metadata| {
                    identity != Some((metadata.dev(), metadata.ino())) || metadata.len() < position
                });
                if replaced_or_truncated {
                    reader = None;
                    identity = None;
                    position = 0;
                } else {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Ok(bytes) => {
                position = position.saturating_add(bytes as u64);
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    continue;
                }
                match kind {
                    ServerLogKind::Output => info!(target: "nwnrs::server", "{line}"),
                    ServerLogKind::Error => {
                        tracing::error!(target: "nwnrs::server", "{line}");
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::warn!(target: "nwnrs::launcher", path = %path.display(), %error, "failed to read followed server log");
                reader = None;
                identity = None;
                position = 0;
            }
        }
    }
}

#[cfg(windows)]
fn follow_server_log(path: &Path, kind: ServerLogKind, stop: &AtomicBool) {
    use std::{
        fs::File,
        io::{BufReader, SeekFrom},
        time::Duration,
    };

    let skip_existing = path.is_file();
    let mut first_open = true;
    let mut reader = None;
    let mut position = 0_u64;
    while !stop.load(Ordering::Relaxed) {
        if reader.is_none() {
            match File::open(path) {
                Ok(file) => {
                    let mut new_reader = BufReader::new(file);
                    let start = if first_open && skip_existing {
                        SeekFrom::End(0)
                    } else {
                        SeekFrom::Start(0)
                    };
                    match new_reader.seek(start) {
                        Ok(offset) => position = offset,
                        Err(error) => {
                            tracing::warn!(target: "nwnrs::launcher", path = %path.display(), %error, "failed to seek followed server log");
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                    }
                    reader = Some(new_reader);
                    first_open = false;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(error) => {
                    tracing::warn!(target: "nwnrs::launcher", path = %path.display(), %error, "failed to open followed server log");
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }
            }
        }

        let Some(active_reader) = reader.as_mut() else {
            continue;
        };
        let mut line = String::new();
        match active_reader.read_line(&mut line) {
            Ok(0) => {
                if fs::metadata(path).is_ok_and(|metadata| metadata.len() < position) {
                    reader = None;
                    position = 0;
                } else {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Ok(bytes) => {
                position = position.saturating_add(bytes as u64);
                let line = line.trim_end_matches(['\r', '\n']);
                if line.is_empty() {
                    continue;
                }
                match kind {
                    ServerLogKind::Output => info!(target: "nwnrs::server", "{line}"),
                    ServerLogKind::Error => tracing::error!(target: "nwnrs::server", "{line}"),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                tracing::warn!(target: "nwnrs::launcher", path = %path.display(), %error, "failed to read followed server log");
                reader = None;
                position = 0;
            }
        }
    }
}

#[cfg(unix)]
fn start_terminal_input_relay(sender: std::sync::mpsc::Sender<ServerInput>) {
    std::thread::spawn(move || {
        use tracing::warn;

        let mut input = std::io::stdin().lock();
        let mut buffer = [0_u8; 1024];
        loop {
            match input.read(&mut buffer) {
                Ok(0) => {
                    let _ = sender.send(ServerInput::End);
                    break;
                }
                Ok(count) => {
                    let Some(bytes) = buffer.get(..count) else {
                        warn!(count, "terminal input returned an invalid byte count");
                        break;
                    };
                    if sender.send(ServerInput::Data(bytes.to_vec())).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    warn!(%error, "failed to read terminal input");
                    let _ = sender.send(ServerInput::End);
                    break;
                }
            }
        }
    });
}

#[cfg(windows)]
fn start_windows_terminal_input_relay(
    input_sender: std::sync::mpsc::Sender<ServerInput>,
    shutdown_sender: tokio::sync::mpsc::UnboundedSender<()>,
) {
    std::thread::spawn(move || {
        use tracing::warn;

        let mut input = std::io::stdin().lock();
        loop {
            let mut line = Vec::new();
            match input.read_until(b'\n', &mut line) {
                Ok(0) => {
                    let _ = input_sender.send(ServerInput::End);
                    break;
                }
                Ok(_) => {
                    let command = line.strip_suffix(b"\n").unwrap_or(&line);
                    let command = command.strip_suffix(b"\r").unwrap_or(command);
                    if command.eq_ignore_ascii_case(b"quit") {
                        if shutdown_sender.send(()).is_err() {
                            break;
                        }
                    } else if input_sender.send(ServerInput::Data(line)).is_err() {
                        break;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    warn!(%error, "failed to read terminal input");
                    let _ = input_sender.send(ServerInput::End);
                    break;
                }
            }
        }
    });
}

#[cfg(any(unix, windows))]
fn start_server_console_relay(
    server_output: std::process::ChildStdout,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let output = std::io::BufReader::new(server_output);
        for line in output.lines() {
            match line {
                Ok(line) if line.is_empty() => {}
                Ok(line) => info!(target: "nwnrs::console", "{line}"),
                Err(error) => {
                    tracing::warn!(
                        target: "nwnrs::launcher",
                        %error,
                        "failed to read NWServer console output"
                    );
                    break;
                }
            }
        }
    })
}

#[cfg(any(unix, windows))]
fn start_server_error_relay(
    server_error: std::process::ChildStderr,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let output = std::io::BufReader::new(server_error);
        for line in output.lines() {
            match line {
                Ok(line) if line.is_empty() => {}
                Ok(line) => relay_server_error_line(&line),
                Err(error) => {
                    tracing::warn!(
                        target: "nwnrs::launcher",
                        %error,
                        "failed to read NWServer error output"
                    );
                    break;
                }
            }
        }
    })
}

#[cfg(any(unix, windows))]
fn relay_server_error_line(line: &str) {
    if let Some(message) = line.strip_prefix(" INFO nwnrs::runtime: ") {
        info!(target: "nwnrs::runtime", "{message}");
    } else if let Some(message) = line.strip_prefix(" WARN nwnrs::runtime: ") {
        tracing::warn!(target: "nwnrs::runtime", "{message}");
    } else if let Some(message) = line.strip_prefix("ERROR nwnrs::runtime: ") {
        tracing::error!(target: "nwnrs::runtime", "{message}");
    } else if let Some(message) = line.strip_prefix("DEBUG nwnrs::runtime: ") {
        tracing::debug!(target: "nwnrs::runtime", "{message}");
    } else if let Some(message) = line.strip_prefix("TRACE nwnrs::runtime: ") {
        tracing::trace!(target: "nwnrs::runtime", "{message}");
    } else if let Some(message) = line.strip_prefix(" INFO nwnrs::script: ") {
        info!(target: "nwnrs::script", "{message}");
    } else if let Some(message) = line.strip_prefix(" WARN nwnrs::script: ") {
        tracing::warn!(target: "nwnrs::script", "{message}");
    } else if let Some(message) = line.strip_prefix("ERROR nwnrs::script: ") {
        tracing::error!(target: "nwnrs::script", "{message}");
    } else if let Some(message) = line.strip_prefix("DEBUG nwnrs::script: ") {
        tracing::debug!(target: "nwnrs::script", "{message}");
    } else if let Some(message) = line.strip_prefix("TRACE nwnrs::script: ") {
        tracing::trace!(target: "nwnrs::script", "{message}");
    } else {
        info!(target: "nwnrs::console", "{line}");
    }
}

#[cfg(any(unix, windows))]
fn start_server_input_writer(
    mut server_input: std::process::ChildStdin,
    receiver: std::sync::mpsc::Receiver<ServerInput>,
) -> std::thread::JoinHandle<()> {
    use tracing::warn;

    std::thread::spawn(move || {
        while let Ok(input) = receiver.recv() {
            let input_ended = matches!(input, ServerInput::End | ServerInput::Shutdown);
            let result = match input {
                ServerInput::Data(bytes) => server_input.write_all(&bytes),
                ServerInput::End => Ok(()),
                ServerInput::Shutdown => {
                    tracing::debug!(
                        target: "nwnrs::launcher",
                        "writing native quit command to NWServer"
                    );
                    server_input
                        .write_all(b"quit\n")
                        .and_then(|()| server_input.flush())
                }
            };
            if let Err(error) = result {
                warn!(%error, "failed to write to server standard input");
                break;
            }
            if input_ended {
                break;
            }
        }
    })
}

#[cfg(unix)]
fn request_server_shutdown(
    server: &std::process::Child,
    input_sender: &std::sync::mpsc::Sender<ServerInput>,
    shutdown_requested_at: &mut Option<std::time::Instant>,
) {
    use nix::sys::signal::Signal;

    if let Some(requested_at) = *shutdown_requested_at {
        if requested_at.elapsed() < DUPLICATE_SHUTDOWN_SIGNAL_WINDOW {
            tracing::debug!(
                target: "nwnrs::launcher",
                "ignoring duplicate shutdown signal"
            );
            return;
        }
        tracing::warn!(
            target: "nwnrs::launcher",
            "forcing NWServer to terminate after repeated shutdown request"
        );
        forward_signal(server, Signal::SIGKILL);
        return;
    }
    *shutdown_requested_at = Some(std::time::Instant::now());
    info!(
        target: "nwnrs::launcher",
        "requesting graceful NWServer shutdown"
    );
    if input_sender.send(ServerInput::Shutdown).is_err() {
        tracing::warn!(
            target: "nwnrs::launcher",
            "server input relay is unavailable; falling back to TERM"
        );
        forward_signal(server, Signal::SIGTERM);
    }
}

#[cfg(unix)]
fn forward_signal(server: &std::process::Child, signal: nix::sys::signal::Signal) {
    use nix::{errno::Errno, sys::signal::killpg, unistd::Pid};
    use tracing::warn;

    let process_id = server.id();
    let Ok(process_id) = i32::try_from(process_id) else {
        warn!(
            process_id,
            ?signal,
            "server process ID cannot be represented by the host"
        );
        return;
    };
    if let Err(error) = killpg(Pid::from_raw(process_id), signal)
        && error != Errno::ESRCH
    {
        warn!(process_id, ?signal, %error, "failed to forward signal to server");
    }
}

#[cfg(any(unix, windows))]
fn stop_log_followers(log_followers: &mut Vec<LogFollower>) {
    while let Some(mut follower) = log_followers.pop() {
        follower.stop.store(true, Ordering::Relaxed);
        if let Some(relay) = follower.relay.take()
            && relay.join().is_err()
        {
            tracing::warn!(target: "nwnrs::launcher", "server log follower thread panicked");
        }
    }
}

#[cfg(unix)]
fn exit_status_code(status: std::process::ExitStatus) -> ExitCode {
    ExitCode::from(exit_status_value(status))
}

#[cfg(unix)]
fn exit_status_value(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt as _;

    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .or_else(|| {
            status
                .signal()
                .and_then(|signal| u8::try_from(signal).ok())
                .and_then(|signal| 128_u8.checked_add(signal))
        })
        .unwrap_or(1)
}

#[cfg(windows)]
fn windows_exit_status_code(status: std::process::ExitStatus) -> ExitCode {
    ExitCode::from(
        status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsStr,
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use nwnrs_runtime::{
        AbiLayouts, Architecture, BinaryIdentity, BridgeTarget, CExoStringLayout, ENV_ENABLED,
        ENV_REQUIRED, ENV_SUPERVISED, ENV_TARGET_PACK, EVENT_CONTEXT_CAPABILITY_VERSION,
        EngineClassLayouts, EventTarget, NWSCRIPT_BRIDGE_CAPABILITY_VERSION, OperatingSystem,
        Platform, PlayerListLayout, RUNTIME_API_VERSION, SERVER_STATE_CAPABILITY_VERSION,
        ServerStateTarget, TARGET_PACK_SCHEMA_VERSION, TargetAddress, TargetPack, TargetServer,
        TargetSource, VectorLayout,
    };

    #[cfg(feature = "tooling")]
    use super::{
        DEFAULT_DOCKER_HOME, DEFAULT_DOCKER_IMAGE, DockerLaunchPlan, validate_native_options,
    };
    use super::{LINUX_PRELOAD, LaunchPlan, MACOS_PRELOAD, server_log_paths};
    #[cfg(unix)]
    use super::{
        ServerInput, exit_status_value, forward_signal, isolate_process_group,
        request_server_shutdown, start_server_input_writer,
    };
    use crate::args::{ColorMode, RunCmd};

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    #[cfg(feature = "tooling")]
    #[test]
    fn prepares_attached_docker_run_with_managed_defaults() {
        let command = RunCmd {
            container:         false,
            docker:            true,
            docker_image:      DEFAULT_DOCKER_IMAGE.to_string(),
            docker_name:       Some("test-server".to_string()),
            docker_home:       DEFAULT_DOCKER_HOME.to_string(),
            docker_publish:    Vec::new(),
            docker_arg:        vec!["network=host".to_string()],
            color:             ColorMode::Always,
            no_tail_logs:      true,
            runtime:           None,
            targets:           None,
            working_directory: None,
            arguments:         vec!["-module".to_string(), "custom".to_string()],
        };

        let plan = DockerLaunchPlan::from_command(command, true, true);
        assert_eq!(plan.image, DEFAULT_DOCKER_IMAGE);
        assert_eq!(
            plan.args,
            vec![
                "run",
                "--rm",
                "--read-only",
                "--cap-drop=ALL",
                "--security-opt=no-new-privileges=true",
                "--tmpfs=/nwn/run:uid=1000,gid=0,mode=0770",
                "--tmpfs=/tmp:uid=1000,gid=0,mode=1777",
                "--pull=never",
                "--interactive",
                "--tty",
                "--name",
                "test-server",
                "--publish",
                "5121:5121/udp",
                "--volume",
                "nwserver-home:/nwn/home",
                "--env",
                "NWNRS_COLOR=always",
                "--env",
                "NWN_TAIL_LOGS=n",
                "--network=host",
                DEFAULT_DOCKER_IMAGE,
                "-module",
                "custom",
            ]
        );
    }

    #[cfg(feature = "tooling")]
    #[test]
    fn native_mode_rejects_docker_options_without_flag() {
        let command = RunCmd {
            container:         false,
            docker:            false,
            docker_image:      DEFAULT_DOCKER_IMAGE.to_string(),
            docker_name:       Some("unexpected".to_string()),
            docker_home:       DEFAULT_DOCKER_HOME.to_string(),
            docker_publish:    Vec::new(),
            docker_arg:        Vec::new(),
            color:             ColorMode::Auto,
            no_tail_logs:      false,
            runtime:           None,
            targets:           None,
            working_directory: None,
            arguments:         Vec::new(),
        };

        assert_eq!(
            validate_native_options(&command),
            Err("Docker options require --docker".to_string())
        );
    }

    #[test]
    fn prepares_clean_native_launch_environment() -> Result<(), Box<dyn std::error::Error>> {
        let root = test_directory();
        fs::create_dir_all(&root)?;
        let server = root.join("nwserver");
        let runtime = root.join("libnwnrs_runtime");
        write_host_binary(&server)?;
        write_host_binary(&runtime)?;
        let identity = BinaryIdentity::read(&server)?;
        write_target_pack(&root, &identity)?;

        let plan = LaunchPlan::prepare(RunCmd {
            container: false,
            #[cfg(feature = "tooling")]
            docker: false,
            #[cfg(feature = "tooling")]
            docker_image: DEFAULT_DOCKER_IMAGE.to_string(),
            #[cfg(feature = "tooling")]
            docker_name: None,
            #[cfg(feature = "tooling")]
            docker_home: DEFAULT_DOCKER_HOME.to_string(),
            #[cfg(feature = "tooling")]
            docker_publish: Vec::new(),
            #[cfg(feature = "tooling")]
            docker_arg: Vec::new(),
            color: ColorMode::Never,
            no_tail_logs: true,
            runtime: Some(runtime),
            targets: Some(root.clone()),
            working_directory: Some(root.clone()),
            arguments: vec![
                server.to_string_lossy().into_owned(),
                "-module".to_string(),
                "nwnrs".to_string(),
            ],
        })?;
        let command = plan.command();
        let canonical_root = fs::canonicalize(&root)?;
        assert_eq!(command.get_current_dir(), Some(canonical_root.as_path()));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("-module"), OsStr::new("nwnrs")]
        );
        assert_eq!(environment(&command, ENV_ENABLED), Some(OsStr::new("1")));
        assert_eq!(environment(&command, ENV_REQUIRED), Some(OsStr::new("1")));
        assert_eq!(environment(&command, ENV_SUPERVISED), Some(OsStr::new("1")));
        assert!(environment(&command, ENV_TARGET_PACK).is_some());
        match Platform::host()?.os {
            OperatingSystem::Macos => assert!(environment(&command, MACOS_PRELOAD).is_some()),
            OperatingSystem::Linux => assert!(environment(&command, LINUX_PRELOAD).is_some()),
            OperatingSystem::Windows => {
                assert!(environment(&command, MACOS_PRELOAD).is_none());
                assert!(environment(&command, LINUX_PRELOAD).is_none());
            }
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn derives_logs_from_last_forwarded_user_directory() -> Result<(), String> {
        let working_directory = Path::new("/srv/nwn");
        let paths = server_log_paths(
            &[
                "-userdirectory".to_string(),
                "old-home".to_string(),
                "-USERDIRECTORY=/var/lib/nwn".to_string(),
            ],
            working_directory,
        )?;

        assert_eq!(
            paths,
            [
                PathBuf::from("/var/lib/nwn/logs.0/nwserverLog1.txt"),
                PathBuf::from("/var/lib/nwn/logs.0/nwserverError1.txt"),
            ]
        );
        Ok(())
    }

    #[test]
    fn resolves_relative_user_directory_from_server_working_directory() -> Result<(), String> {
        let paths = server_log_paths(
            &["-userdirectory".to_string(), "server-home".to_string()],
            Path::new("/srv/nwn/bin"),
        )?;

        assert_eq!(
            paths,
            [
                PathBuf::from("/srv/nwn/bin/server-home/logs.0/nwserverLog1.txt"),
                PathBuf::from("/srv/nwn/bin/server-home/logs.0/nwserverError1.txt"),
            ]
        );
        Ok(())
    }

    #[test]
    fn rejects_user_directory_without_a_value() -> Result<(), String> {
        let error = server_log_paths(&["-userdirectory".to_string()], Path::new("/srv/nwn"))
            .err()
            .ok_or_else(|| "missing value unexpectedly succeeded".to_string())?;
        assert!(error.contains("missing its directory"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn preserves_the_server_exit_status() -> Result<(), Box<dyn std::error::Error>> {
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 37")
            .status()?;
        assert_eq!(exit_status_value(status), 37);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn forwards_termination_to_the_server() -> Result<(), Box<dyn std::error::Error>> {
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg("trap 'exit 23' TERM; while :; do sleep 1; done");
        isolate_process_group(&mut command);
        let mut child = command.spawn()?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        forward_signal(&child, nix::sys::signal::Signal::SIGTERM);
        let status = child.wait()?;

        assert_eq!(exit_status_value(status), 23);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn graceful_shutdown_writes_the_native_quit_command() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg("read command; test \"$command\" = quit")
            .stdin(std::process::Stdio::piped());
        isolate_process_group(&mut command);
        let mut child = command.spawn()?;
        let child_input = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("test child did not expose standard input"))?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let writer = start_server_input_writer(child_input, receiver);
        let mut shutdown_requested_at = None;
        request_server_shutdown(&child, &sender, &mut shutdown_requested_at);
        let status = child.wait()?;
        let _ = sender.send(ServerInput::End);
        let _ = writer.join();

        assert!(status.success());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn ignores_an_immediate_duplicate_shutdown_signal() -> Result<(), Box<dyn std::error::Error>> {
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg("read command; test \"$command\" = quit; sleep 0.1")
            .stdin(std::process::Stdio::piped());
        isolate_process_group(&mut command);
        let mut child = command.spawn()?;
        let child_input = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("test child did not expose standard input"))?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let writer = start_server_input_writer(child_input, receiver);
        let mut shutdown_requested_at = None;
        request_server_shutdown(&child, &sender, &mut shutdown_requested_at);
        request_server_shutdown(&child, &sender, &mut shutdown_requested_at);
        let status = child.wait()?;
        let _ = sender.send(ServerInput::End);
        let _ = writer.join();

        assert!(status.success());
        Ok(())
    }

    fn environment<'a>(command: &'a std::process::Command, name: &str) -> Option<&'a OsStr> {
        command
            .get_envs()
            .find_map(|(key, value)| (key == name).then_some(value).flatten())
    }

    fn write_host_binary(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let platform = Platform::host()?;
        let mut bytes = vec![0_u8; 0xa0];
        match platform.os {
            OperatingSystem::Macos => {
                bytes
                    .get_mut(..4)
                    .ok_or("Mach-O magic range")?
                    .copy_from_slice(b"\xcf\xfa\xed\xfe");
                let cpu_type = match platform.architecture {
                    Architecture::Aarch64 => 0x0100_000c_u32,
                    Architecture::X86_64 => 0x0100_0007_u32,
                };
                bytes
                    .get_mut(4..8)
                    .ok_or("Mach-O CPU range")?
                    .copy_from_slice(&cpu_type.to_le_bytes());
            }
            OperatingSystem::Linux => {
                bytes
                    .get_mut(..4)
                    .ok_or("ELF magic range")?
                    .copy_from_slice(b"\x7fELF");
                *bytes.get_mut(4).ok_or("ELF class byte")? = 2;
                *bytes.get_mut(5).ok_or("ELF byte-order byte")? = 1;
                let machine = match platform.architecture {
                    Architecture::Aarch64 => 183_u16,
                    Architecture::X86_64 => 62_u16,
                };
                bytes
                    .get_mut(18..20)
                    .ok_or("ELF machine range")?
                    .copy_from_slice(&machine.to_le_bytes());
            }
            OperatingSystem::Windows => {
                bytes
                    .get_mut(..2)
                    .ok_or("DOS magic range")?
                    .copy_from_slice(b"MZ");
                bytes
                    .get_mut(60..64)
                    .ok_or("PE offset range")?
                    .copy_from_slice(&0x80_u32.to_le_bytes());
                bytes
                    .get_mut(0x80..0x84)
                    .ok_or("PE signature range")?
                    .copy_from_slice(b"PE\0\0");
                let machine = match platform.architecture {
                    Architecture::Aarch64 => 0xaa64_u16,
                    Architecture::X86_64 => 0x8664_u16,
                };
                bytes
                    .get_mut(0x84..0x86)
                    .ok_or("PE machine range")?
                    .copy_from_slice(&machine.to_le_bytes());
                bytes
                    .get_mut(0x98..0x9a)
                    .ok_or("PE optional magic range")?
                    .copy_from_slice(&0x020b_u16.to_le_bytes());
            }
        }
        fs::write(path, bytes)?;
        Ok(())
    }

    fn write_target_pack(
        root: &Path,
        identity: &BinaryIdentity,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let platform = identity.platform;
        let pack = TargetPack {
            schema_version: TARGET_PACK_SCHEMA_VERSION,
            runtime_api:    RUNTIME_API_VERSION,
            server:         TargetServer {
                sha256: identity.sha256.to_string(),
                platform,
                build: Some("fixture".to_string()),
            },
            source:         target_source(),
            layouts:        target_layouts(),
            bridge:         bridge_target(),
            server_state:   Some(server_state_target()),
            administration: None,
            events:         Some(event_target()),
        };
        let directory = root.join(platform.directory_name());
        fs::create_dir_all(&directory)?;
        fs::write(
            directory.join(format!("{}.toml", identity.sha256)),
            toml::to_string(&pack)?,
        )?;
        Ok(())
    }

    fn bridge_target() -> BridgeTarget {
        let address = || TargetAddress::Offset {
            offset: 1
        };
        BridgeTarget {
            version:                NWSCRIPT_BRIDGE_CAPABILITY_VERSION,
            function_management:    address(),
            stack_pop_integer:      address(),
            stack_push_integer:     address(),
            stack_pop_float:        address(),
            stack_push_float:       address(),
            stack_pop_object:       address(),
            stack_push_object:      address(),
            stack_pop_string:       address(),
            stack_push_string:      address(),
            stack_pop_vector:       address(),
            stack_push_vector:      address(),
            free_exo_string_buffer: address(),
        }
    }

    fn server_state_target() -> ServerStateTarget {
        let address = || TargetAddress::Offset {
            offset: 1
        };
        ServerStateTarget {
            version:                 SERVER_STATE_CAPABILITY_VERSION,
            app_manager:             address(),
            get_server_info:         address(),
            get_player_list:         address(),
            get_net_layer:           address(),
            get_session_max_players: address(),
            get_udp_port:            address(),
        }
    }

    fn event_target() -> EventTarget {
        EventTarget {
            version: EVENT_CONTEXT_CAPABILITY_VERSION,
        }
    }

    fn target_source() -> TargetSource {
        TargetSource {
            unified_commit: "3d4c4e13c6bf01b032ffe90534fc4a19eb036c03".to_string(),
            nwn_build:      8193,
            nwn_revision:   37,
            nwn_postfix:    17,
        }
    }

    fn target_layouts() -> AbiLayouts {
        AbiLayouts {
            c_exo_string: CExoStringLayout {
                size:                 16,
                alignment:            8,
                string_offset:        0,
                string_length_offset: 8,
                buffer_length_offset: 12,
            },
            player_list:  PlayerListLayout {
                size:            16,
                alignment:       8,
                elements_offset: 0,
                count_offset:    8,
                capacity_offset: 12,
            },
            vector:       VectorLayout {
                size:      12,
                alignment: 4,
                x_offset:  0,
                y_offset:  4,
                z_offset:  8,
            },
            classes:      EngineClassLayouts {
                command_implementer_vm_offset: 0,
                app_manager_server_offset: 8,
                server_info_module_offset: 8,
                server_info_joining_restrictions_offset: 136,
                server_info_play_options_offset: 252,
                server_info_persistent_world_options_offset: 404,
                persistent_world_options_server_vault_by_player_name_offset: 16,
                joining_restrictions_min_level_offset: 104,
                joining_restrictions_max_level_offset: 108,
                server_exo_app_internal_offset: 8,
                internal_banned_ip_list_offset: 65920,
                internal_banned_cd_key_list_offset: 65936,
                internal_banned_player_name_list_offset: 65952,
                module_turd_list_offset: 112,
                player_turd_community_name_offset: if cfg!(target_os = "linux") {
                    768
                } else {
                    752
                },
                player_turd_first_name_offset: if cfg!(target_os = "linux") { 784 } else { 768 },
                player_turd_last_name_offset: if cfg!(target_os = "linux") { 800 } else { 784 },
                linked_list_head_offset: 0,
                linked_list_count_offset: 16,
                linked_list_node_next_offset: 8,
                linked_list_node_object_offset: 16,
                player_id_offset: 72,
                player_file_name_offset: 181,
                player_file_name_size: 17,
                net_layer_player_info_cd_key_offset: 136,
                player_cd_key_public_offset: 0,
                exo_base_alias_list_offset: 32,
                creature_stats_offset: if cfg!(target_os = "linux") {
                    2824
                } else {
                    2760
                },
                creature_stats_first_name_offset: 72,
                creature_stats_last_name_offset: 88,
                vm_recursion_level_offset: 36,
                vm_script_array_offset: 40,
                vm_script_slot_count: 8,
                vm_script_size: if cfg!(target_os = "linux") { 152 } else { 136 },
                vm_script_alignment: 8,
                vm_script_name_offset: 24,
                vm_script_event_id_offset: 72,
            },
        }
    }

    fn test_directory() -> PathBuf {
        let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "nwnrs-launcher-test-{}-{sequence}",
            std::process::id()
        ))
    }
}
