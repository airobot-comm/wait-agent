use crate::application::target_registry_service::{
    DefaultTargetCatalogGateway, TargetRegistryService,
};
use crate::application::workspace_service::{BootstrappedWorkspace, WorkspaceService};
use crate::domain::session_catalog::{ManagedSessionRecord, SessionTransport};
use crate::domain::workspace::WorkspaceInstanceConfig;
use crate::infra::error_log::ERROR_LOG;
use crate::infra::tmux::{EmbeddedTmuxBackend, TmuxError, TmuxLayoutGateway, TmuxSocketName};
use crate::lifecycle::LifecycleError;
#[cfg(test)]
use crate::runtime::current_executable::current_waitagent_executable;
use crate::runtime::local_target_host_runtime::local_target_host_program;
use crate::runtime::remote_target_publication_runtime::RemoteTargetPublicationRuntime;
use crate::runtime::workspace_runtime::WorkspaceRuntime;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

const WAITAGENT_MAIN_PANE_OPTION: &str = "@waitagent_main_pane_id";
const WAITAGENT_PANE_TARGET_SESSION_OPTION: &str = "@waitagent_target_session_name";

pub struct TargetHostRuntime {
    workspace_runtime: WorkspaceRuntime<EmbeddedTmuxBackend>,
    backend: EmbeddedTmuxBackend,
    remote_target_publication_runtime: RemoteTargetPublicationRuntime,
    target_registry: TargetRegistryService<DefaultTargetCatalogGateway>,
    current_executable: PathBuf,
    network: crate::cli::RemoteNetworkConfig,
}

impl TargetHostRuntime {
    #[cfg(test)]
    pub fn from_build_env(backend: EmbeddedTmuxBackend) -> Result<Self, LifecycleError> {
        let current_executable = current_waitagent_executable()?;
        Self::from_build_env_with_network_and_executable(
            backend,
            crate::cli::RemoteNetworkConfig::default(),
            current_executable,
        )
    }

    pub fn new(
        workspace_runtime: WorkspaceRuntime<EmbeddedTmuxBackend>,
        backend: EmbeddedTmuxBackend,
        remote_target_publication_runtime: RemoteTargetPublicationRuntime,
        target_registry: TargetRegistryService<DefaultTargetCatalogGateway>,
        current_executable: PathBuf,
        network: crate::cli::RemoteNetworkConfig,
    ) -> Self {
        Self {
            workspace_runtime,
            backend,
            remote_target_publication_runtime,
            target_registry,
            current_executable,
            network,
        }
    }

    pub fn from_build_env_with_network_and_executable(
        backend: EmbeddedTmuxBackend,
        network: crate::cli::RemoteNetworkConfig,
        current_executable: PathBuf,
    ) -> Result<Self, LifecycleError> {
        Ok(Self::new(
            WorkspaceRuntime::new(WorkspaceService::new(backend.clone())),
            backend,
            RemoteTargetPublicationRuntime::from_build_env_with_network(network.clone())?,
            TargetRegistryService::new(
                DefaultTargetCatalogGateway::from_build_env_with_network(network.clone())
                    .map_err(target_host_error)?,
            ),
            current_executable,
            network,
        ))
    }

    pub fn ensure_target_host(
        &self,
        config: WorkspaceInstanceConfig,
    ) -> Result<BootstrappedWorkspace, TmuxError> {
        let t_total = Instant::now();
        let workspace = self.workspace_runtime.ensure_workspace_for_config(config)?;
        ERROR_LOG.log(format!(
            "[diag-newhost] target_host ensure_workspace socket={} session={} elapsed={:?}",
            workspace.workspace_handle.socket_name.as_str(),
            workspace.workspace_handle.session_name.as_str(),
            t_total.elapsed()
        ));
        let program = local_target_host_program(
            &self.current_executable,
            workspace.workspace_handle.socket_name.as_str(),
            workspace.workspace_handle.session_name.as_str(),
            &workspace.workspace_dir,
            &self.network,
        );
        let t_pane = Instant::now();
        let pane = self.backend.target_main_pane_on_socket(
            workspace.workspace_handle.socket_name.as_str(),
            workspace.workspace_handle.session_name.as_str(),
        )?;
        ERROR_LOG.log(format!(
            "[diag-newhost] target_host target_main_pane socket={} session={} pane={} elapsed={:?} total={:?}",
            workspace.workspace_handle.socket_name.as_str(),
            workspace.workspace_handle.session_name.as_str(),
            pane.as_str(),
            t_pane.elapsed(),
            t_total.elapsed()
        ));
        let t_respawn = Instant::now();
        self.backend
            .respawn_pane(&workspace.workspace_handle, &pane, &program)?;
        ERROR_LOG.log(format!(
            "[diag-newhost] target_host respawn_pane socket={} session={} pane={} elapsed={:?} total={:?}",
            workspace.workspace_handle.socket_name.as_str(),
            workspace.workspace_handle.session_name.as_str(),
            pane.as_str(),
            t_respawn.elapsed(),
            t_total.elapsed()
        ));
        let t_metadata = Instant::now();
        self.backend.set_pane_option(
            &workspace.workspace_handle,
            &pane,
            WAITAGENT_PANE_TARGET_SESSION_OPTION,
            workspace.workspace_handle.session_name.as_str(),
        )?;
        self.backend.set_session_option(
            &workspace.workspace_handle,
            WAITAGENT_MAIN_PANE_OPTION,
            pane.as_str(),
        )?;
        ERROR_LOG.log(format!(
            "[diag-newhost] target_host write_metadata socket={} session={} pane={} elapsed={:?} total={:?}",
            workspace.workspace_handle.socket_name.as_str(),
            workspace.workspace_handle.session_name.as_str(),
            pane.as_str(),
            t_metadata.elapsed(),
            t_total.elapsed()
        ));
        Ok(workspace)
    }

    pub fn refresh_published_target_session(
        &self,
        session: Option<&ManagedSessionRecord>,
    ) -> Result<(), LifecycleError> {
        let Some(session) = session.filter(|session| session.is_target_host()) else {
            return Ok(());
        };
        self.remote_target_publication_runtime
            .signal_source_session_refresh(
                session.address.server_id(),
                session.address.session_id(),
            )
    }

    pub fn close_target_session_identity(
        &self,
        target: Option<&str>,
    ) -> Result<(), LifecycleError> {
        let Some(target) = target else {
            return Ok(());
        };
        if let Some(session) = self
            .target_registry
            .find_target(target)
            .map_err(target_host_error)?
        {
            return self.close_resolved_target_session(&session);
        }
        if let Some((socket_name, session_name)) = split_qualified_target(target) {
            self.remote_target_publication_runtime
                .signal_source_session_closed(socket_name, session_name)?;
            return match self.backend.run_socket_command(
                &TmuxSocketName::new(socket_name),
                &[
                    "kill-session".to_string(),
                    "-t".to_string(),
                    session_name.to_string(),
                ],
            ) {
                Ok(()) => Ok(()),
                Err(error) if error.is_command_failure() => Ok(()),
                Err(error) => Err(target_host_error(error)),
            };
        }
        Ok(())
    }

    fn close_resolved_target_session(
        &self,
        session: &ManagedSessionRecord,
    ) -> Result<(), LifecycleError> {
        if !session.is_target_host() {
            return Ok(());
        }
        if session.address.transport() == &SessionTransport::RemotePeer {
            self.remote_target_publication_runtime
                .signal_source_session_closed(
                    session.address.server_id(),
                    session.address.session_id(),
                )?;
            return Ok(());
        }
        self.remote_target_publication_runtime
            .signal_source_session_closed(
                session.address.server_id(),
                session.address.session_id(),
            )?;
        self.backend
            .run_socket_command(
                &TmuxSocketName::new(session.address.server_id()),
                &[
                    "kill-session".to_string(),
                    "-t".to_string(),
                    session.address.session_id().to_string(),
                ],
            )
            .map_err(target_host_error)
    }
}

fn split_qualified_target(target: &str) -> Option<(&str, &str)> {
    let (socket_name, session_name) = target.rsplit_once(':')?;
    if socket_name.is_empty() || session_name.is_empty() {
        return None;
    }
    Some((socket_name, session_name))
}

fn target_host_error(error: TmuxError) -> LifecycleError {
    LifecycleError::Io(
        "tmux-native target-host command failed".to_string(),
        io::Error::new(io::ErrorKind::Other, error.to_string()),
    )
}
