use crate::domain::session_catalog::ManagedSessionTaskState;
use crate::infra::tmux_error::{parse_tmux_id, TmuxError};
use crate::infra::tmux_types::{TmuxPaneId, TmuxPaneInfo, TmuxSocketName};
use std::path::PathBuf;

use super::EmbeddedTmuxBackend;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TmuxSessionRuntimeMetadata {
    pub(super) command_name: Option<String>,
    pub(super) current_path: Option<PathBuf>,
    pub(super) task_state: ManagedSessionTaskState,
    pub(super) is_dead: bool,
}

impl EmbeddedTmuxBackend {
    pub(super) fn session_runtime_metadata(
        &self,
        socket_name: &TmuxSocketName,
        session_name: &str,
    ) -> Result<TmuxSessionRuntimeMetadata, TmuxError> {
        let panes = self.list_panes_on_target(socket_name, session_name)?;
        let Some(main_pane) = panes.iter().find(|pane| {
            pane.title != super::WAITAGENT_SIDEBAR_PANE_TITLE
                && pane.title != super::WAITAGENT_FOOTER_PANE_TITLE
        }) else {
            return Ok(TmuxSessionRuntimeMetadata::default());
        };
        let pane_text = self.capture_pane_text(socket_name, &main_pane.pane_id)?;
        let current_command = main_pane.current_command.as_deref().unwrap_or_default();
        let foreground_argv = super::foreground_process_argv_for_pane_shell(main_pane.pane_pid);
        let command_name = self.registry.detect_command_name(
            current_command,
            foreground_argv.as_deref(),
            &pane_text,
        );
        let task_state = if main_pane.in_mode {
            ManagedSessionTaskState::Running
        } else {
            self.registry
                .infer_task_state(Some(&command_name), &pane_text)
        };
        Ok(TmuxSessionRuntimeMetadata {
            command_name: Some(command_name.clone()),
            current_path: main_pane.current_path.clone(),
            task_state,
            is_dead: main_pane.is_dead,
        })
    }

    pub(super) fn list_panes_on_target(
        &self,
        socket_name: &TmuxSocketName,
        target: &str,
    ) -> Result<Vec<TmuxPaneInfo>, TmuxError> {
        let args = vec![
            "list-panes".to_string(),
            "-t".to_string(),
            target.to_string(),
            "-F".to_string(),
            "#{pane_id}\t#{pane_pid}\t#{pane_title}\t#{pane_current_command}\t#{pane_current_path}\t#{pane_dead}\t#{pane_in_mode}"
                .to_string(),
        ];
        let output = self.run_on_socket(socket_name, &args)?;
        output
            .stdout
            .lines()
            .map(Self::pane_info_for_line)
            .collect::<Result<Vec<_>, _>>()
    }

    fn capture_pane_text(
        &self,
        socket_name: &TmuxSocketName,
        pane_id: &TmuxPaneId,
    ) -> Result<String, TmuxError> {
        let args = vec![
            "capture-pane".to_string(),
            "-p".to_string(),
            "-t".to_string(),
            pane_id.as_str().to_string(),
        ];
        let output = self.run_on_socket(socket_name, &args)?;
        Ok(output.stdout)
    }

    pub(super) fn pane_info_for_line(line: &str) -> Result<TmuxPaneInfo, TmuxError> {
        let mut parts = line.splitn(7, '\t');
        let pane_id = parts.next().unwrap_or_default();
        let pane_pid = parts.next().unwrap_or_default();
        let title = parts.next().unwrap_or_default();
        let current_command = parts.next().unwrap_or_default();
        let current_path = parts.next().unwrap_or_default();
        let dead = parts.next().unwrap_or_default();
        let in_mode = parts.next().unwrap_or_default();

        Ok(TmuxPaneInfo {
            pane_id: TmuxPaneId::new(parse_tmux_id(pane_id, '%', "pane id")?),
            pane_pid: pane_pid.parse::<u32>().ok(),
            title: title.to_string(),
            current_command: (!current_command.is_empty()).then(|| current_command.to_string()),
            current_path: (!current_path.is_empty()).then(|| PathBuf::from(current_path)),
            is_dead: dead == "1",
            in_mode: in_mode == "1",
        })
    }
}
