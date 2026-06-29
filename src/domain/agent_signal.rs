use crate::domain::session_catalog::ManagedSessionTaskState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSignalEnvelope {
    pub version: u32,
    pub agent: String,
    pub event: String,
    pub socket: String,
    pub session: String,
    pub pane: String,
    pub token: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStateSource {
    Hook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentStateUpdate {
    pub state: ManagedSessionTaskState,
    pub source: AgentStateSource,
}

pub trait AgentSignalHandler {
    fn handle(&self, signal: &AgentSignalEnvelope) -> Option<AgentStateUpdate>;
}

pub trait AgentSignalHandlerFactory {
    fn create(&self, agent: &str) -> Option<Box<dyn AgentSignalHandler + Send + Sync>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuiltinAgentSignalHandlerFactory;

impl AgentSignalHandlerFactory for BuiltinAgentSignalHandlerFactory {
    fn create(&self, agent: &str) -> Option<Box<dyn AgentSignalHandler + Send + Sync>> {
        match agent {
            "codex" => Some(Box::new(CodexSignalHandler)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CodexSignalHandler;

impl AgentSignalHandler for CodexSignalHandler {
    fn handle(&self, signal: &AgentSignalEnvelope) -> Option<AgentStateUpdate> {
        let state = match signal.event.as_str() {
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => ManagedSessionTaskState::Running,
            "PermissionRequest" => ManagedSessionTaskState::Confirm,
            "Stop" => ManagedSessionTaskState::Input,
            _ => return None,
        };
        Some(AgentStateUpdate {
            state,
            source: AgentStateSource::Hook,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(event: &str) -> AgentSignalEnvelope {
        AgentSignalEnvelope {
            version: 1,
            agent: "codex".to_string(),
            event: event.to_string(),
            socket: "wa-test".to_string(),
            session: "target".to_string(),
            pane: "%1".to_string(),
            token: "secret".to_string(),
            payload: Value::Null,
        }
    }

    #[test]
    fn codex_handler_maps_lifecycle_events_to_states() {
        let handler = CodexSignalHandler;
        assert_eq!(
            handler.handle(&signal("UserPromptSubmit")).map(|u| u.state),
            Some(ManagedSessionTaskState::Running)
        );
        assert_eq!(
            handler
                .handle(&signal("PermissionRequest"))
                .map(|u| u.state),
            Some(ManagedSessionTaskState::Confirm)
        );
        assert_eq!(
            handler.handle(&signal("PreToolUse")).map(|u| u.state),
            Some(ManagedSessionTaskState::Running)
        );
        assert_eq!(
            handler.handle(&signal("PostToolUse")).map(|u| u.state),
            Some(ManagedSessionTaskState::Running)
        );
        assert_eq!(
            handler.handle(&signal("Stop")).map(|u| u.state),
            Some(ManagedSessionTaskState::Input)
        );
    }

    #[test]
    fn codex_handler_ignores_unknown_events() {
        assert!(CodexSignalHandler.handle(&signal("SessionStart")).is_none());
    }

    #[test]
    fn builtin_factory_creates_codex_handler_only() {
        let factory = BuiltinAgentSignalHandlerFactory;
        assert!(factory.create("codex").is_some());
        assert!(factory.create("unknown").is_none());
    }
}
