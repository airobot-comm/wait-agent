use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use crate::infra::tmux::{EmbeddedTmuxBackend, TmuxSocketName};

const POLL_INTERVAL: Duration = Duration::from_millis(1200);
const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Default)]
struct PaneActivityState {
    total_bytes: u64,
    current_command: String,
}

pub(crate) fn spawn_pane_activity_watcher(
    backend: EmbeddedTmuxBackend,
    socket_name: String,
    session_name: String,
) {
    thread::spawn(move || {
        let mut pane_states: HashMap<String, PaneActivityState> = HashMap::new();
        let mut last_signal: Option<Instant> = None;
        let socket = TmuxSocketName::new(&socket_name);

        loop {
            thread::sleep(POLL_INTERVAL);

            let panes = match backend.pane_activity_on_socket(&socket) {
                Ok(panes) => panes,
                Err(_) => continue,
            };

            let mut should_signal = false;

            for (bytes, command, title, session_id) in &panes {
                if title == crate::infra::tmux::WAITAGENT_SIDEBAR_PANE_TITLE
                    || title == crate::infra::tmux::WAITAGENT_FOOTER_PANE_TITLE
                {
                    continue;
                }

                let key = format!("{session_id}:{title}");
                let prev = pane_states.get(&key);
                let current = PaneActivityState {
                    total_bytes: *bytes,
                    current_command: command.clone(),
                };

                if let Some(prev) = prev {
                    if current.total_bytes != prev.total_bytes
                        || current.current_command != prev.current_command
                    {
                        should_signal = true;
                    }
                }

                pane_states.insert(key, current);
            }

            if should_signal {
                let now = Instant::now();
                if last_signal.map_or(true, |t| now.duration_since(t) >= DEBOUNCE_INTERVAL) {
                    last_signal = Some(now);
                    let _ = backend.signal_chrome_refresh_on_socket(&socket_name, &session_name);
                }
            }
        }
    });
}
