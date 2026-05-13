use crate::domain::agent_detector::AgentDetector;
use crate::domain::session_catalog::ManagedSessionTaskState;

pub struct ClaudeDetector;

impl AgentDetector for ClaudeDetector {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn detect_from_process(
        &self,
        current_command: &str,
        argv: Option<&[String]>,
    ) -> Option<&'static str> {
        if current_command == "claude" || current_command == "claude.js" {
            return Some("claude");
        }
        // node wrapper
        if current_command == "node" {
            if let Some(argv) = argv {
                let is_claude = argv.iter().skip(1).any(|arg| {
                    std::path::Path::new(arg)
                        .file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        == Some("claude")
                        || std::path::Path::new(arg)
                            .file_name()
                            .and_then(std::ffi::OsStr::to_str)
                            == Some("claude.js")
                });
                if is_claude {
                    return Some("claude");
                }
            }
        }
        None
    }

    fn detect_from_pane_text(
        &self,
        current_command: &str,
        pane_text: &str,
    ) -> Option<&'static str> {
        if !crate::domain::agent_detector::SHELL_NAMES.contains(&current_command) {
            return None;
        }
        let lowered = pane_text.to_ascii_lowercase();
        if lowered.contains("claude") && lowered.contains("type your message") {
            return Some("claude");
        }
        None
    }

    fn infer_task_state(
        &self,
        command_name: Option<&str>,
        pane_text: &str,
    ) -> Option<ManagedSessionTaskState> {
        let command_name = command_name.unwrap_or_default();
        if command_name != "claude" {
            return None;
        }
        let normalized_lines: Vec<&str> = pane_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let last_line = normalized_lines.last().copied().unwrap_or_default();
        let lowered = last_line.to_ascii_lowercase();

        // Confirm — Claude-specific confirmation prompts
        if lowered.contains("run this command")
            || lowered.contains("allow this")
            || lowered.contains("approve this")
            || lowered.ends_with("[y/n]")
            || lowered.ends_with("(y/n)")
        {
            return Some(ManagedSessionTaskState::Confirm);
        }

        // Input — visible prompt or structural indicator
        if last_line.starts_with('›')
            || last_line.starts_with("> ")
            || lowered.contains("ready")
            || lowered.contains("type your message")
            || lowered.contains("send a message")
        {
            return Some(ManagedSessionTaskState::Input);
        }

        Some(ManagedSessionTaskState::Running)
    }
}
