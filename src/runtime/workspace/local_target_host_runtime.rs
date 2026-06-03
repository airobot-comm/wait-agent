use crate::cli::{
    prepend_global_network_args, LocalTargetExitedCommand, LocalTargetHostCommand,
    RemoteNetworkConfig,
};
use crate::infra::error_log::ERROR_LOG;
use crate::infra::tmux::EmbeddedTmuxBackend;
use crate::lifecycle::LifecycleError;
use crate::runtime::remote_target_publication_runtime::RemoteTargetPublicationRuntime;
use crate::runtime::sidecar_process_runtime::spawn_waitagent_sidecar;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub struct LocalTargetHostRuntime {
    backend: EmbeddedTmuxBackend,
    remote_target_publication_runtime: RemoteTargetPublicationRuntime,
    current_executable: PathBuf,
    network: RemoteNetworkConfig,
}

impl LocalTargetHostRuntime {
    pub fn new(
        backend: EmbeddedTmuxBackend,
        remote_target_publication_runtime: RemoteTargetPublicationRuntime,
        current_executable: PathBuf,
        network: RemoteNetworkConfig,
    ) -> Self {
        Self {
            backend,
            remote_target_publication_runtime,
            current_executable,
            network,
        }
    }

    pub fn run_host(&self, command: LocalTargetHostCommand) -> Result<(), LifecycleError> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut child = Command::new(&shell)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                LifecycleError::Io("failed to spawn local target shell".to_string(), error)
            })?;
        let status = child.wait().map_err(|error| {
            LifecycleError::Io("failed to wait for local target shell".to_string(), error)
        })?;
        ERROR_LOG.log(format!(
            "[diag-local-host] shell exited: socket={} target={} status={status}",
            command.socket_name, command.target_session_name
        ));

        let pane_id = std::env::var("TMUX_PANE").unwrap_or_default();
        let args = prepend_global_network_args(
            vec![
                "__local-target-exited".to_string(),
                "--socket-name".to_string(),
                command.socket_name,
                "--target-session-name".to_string(),
                command.target_session_name,
                "--pane-id".to_string(),
                pane_id,
            ],
            &self.network,
        );
        spawn_waitagent_sidecar(&self.current_executable, args).map_err(|error| {
            LifecycleError::Io(
                "failed to spawn local-target-exited sidecar".to_string(),
                error,
            )
        })?;
        Ok(())
    }

    pub fn run_target_exited(
        &self,
        command: LocalTargetExitedCommand,
    ) -> Result<(), LifecycleError> {
        ERROR_LOG.log(format!(
            "[diag-native] run_local_target_exited: socket={} target={} pane={}",
            command.socket_name, command.target_session_name, command.pane_id
        ));
        self.remote_target_publication_runtime
            .signal_source_session_closed(&command.socket_name, &command.target_session_name)?;
        match self.backend.run_socket_command(
            &crate::infra::tmux::TmuxSocketName::new(&command.socket_name),
            &[
                "kill-session".to_string(),
                "-t".to_string(),
                command.target_session_name,
            ],
        ) {
            Ok(()) => Ok(()),
            Err(error) if error.is_command_failure() => Ok(()),
            Err(error) => Err(local_target_host_error(error)),
        }
    }
}

pub(crate) fn local_target_host_program(
    executable: &std::path::Path,
    socket_name: &str,
    target_session_name: &str,
    workspace_dir: &std::path::Path,
    network: &RemoteNetworkConfig,
) -> crate::infra::tmux::TmuxProgram {
    crate::infra::tmux::TmuxProgram::new(executable.display().to_string())
        .with_args(prepend_global_network_args(
            vec![
                "__local-target-host".to_string(),
                "--socket-name".to_string(),
                socket_name.to_string(),
                "--target-session-name".to_string(),
                target_session_name.to_string(),
            ],
            network,
        ))
        .with_start_directory(workspace_dir)
}

fn local_target_host_error(error: crate::infra::tmux::TmuxError) -> LifecycleError {
    LifecycleError::Io(
        "tmux local-target-host command failed".to_string(),
        io::Error::new(io::ErrorKind::Other, error.to_string()),
    )
}
