use crate::domain::agent_detector::AgentDetector;
use crate::domain::session_catalog::ManagedSessionTaskState;

pub struct CodexDetector;

impl AgentDetector for CodexDetector {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn detect_from_process(
        &self,
        current_command: &str,
        argv: Option<&[String]>,
    ) -> Option<&'static str> {
        if current_command == "codex" || current_command == "codex.js" {
            return Some("codex");
        }
        // node wrapper
        if current_command == "node" {
            if let Some(argv) = argv {
                let is_codex = argv.iter().skip(1).any(|arg| {
                    std::path::Path::new(arg)
                        .file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        == Some("codex")
                        || std::path::Path::new(arg)
                            .file_name()
                            .and_then(std::ffi::OsStr::to_str)
                            == Some("codex.js")
                });
                if is_codex {
                    return Some("codex");
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
        if lowered.contains("skip") && lowered.contains("codex") {
            return Some("codex");
        }
        if lowered.contains("type your message")
            || lowered.contains("send a message")
            || lowered.contains("openai codex")
        {
            return Some("codex");
        }
        None
    }

    fn infer_task_state(
        &self,
        command_name: Option<&str>,
        pane_text: &str,
    ) -> Option<ManagedSessionTaskState> {
        let command_name = command_name.unwrap_or_default();
        if command_name != "codex" {
            return None;
        }
        let normalized_lines: Vec<&str> = pane_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let last_line = normalized_lines.last().copied().unwrap_or_default();
        let last_lowered = last_line.to_ascii_lowercase();

        // Confirm — Codex-specific confirmation prompts
        if last_lowered.contains("run this command")
            || last_lowered.contains("allow this")
            || last_lowered.ends_with("[y/n]")
            || last_lowered.ends_with("(y/n)")
        {
            return Some(ManagedSessionTaskState::Confirm);
        }

        // Input — structural prompt indicator or known patterns
        if last_line.starts_with('›')
            || last_line.starts_with("> ")
            || last_lowered.contains("type your message")
            || last_lowered.contains("send a message")
            || last_lowered.contains("tip")
            || last_lowered.contains("ask codex")
        {
            return Some(ManagedSessionTaskState::Input);
        }

        Some(ManagedSessionTaskState::Running)
    }
}
