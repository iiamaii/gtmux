//! Observational hooks only: never print a permission decision or read transcripts.
use clap::{Subcommand, ValueEnum};
use serde_json::{json, Value};
use std::{io::Read, process::ExitCode};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Agent { Claude, Codex, Gemini, Copilot, Cursor, Aider, Opencode }
#[derive(Debug, Subcommand)]
pub enum AgentCmd {
    /// Print additive hook configuration. Merge it with existing hooks; never replace your settings.
    Hooks { #[arg(value_enum)] agent: Agent },
    /// Read a lifecycle event on stdin and report to the enclosing gtmux terminal.
    Event { #[arg(value_enum)] agent: Agent, #[arg(long)] event: Option<String> },
}
fn name(agent: Agent) -> &'static str {
    match agent { Agent::Claude => "claude", Agent::Codex => "codex", Agent::Gemini => "gemini", Agent::Copilot => "copilot", Agent::Cursor => "cursor", Agent::Aider => "aider", Agent::Opencode => "opencode" }
}
pub fn state(agent: Agent, input: &Value, override_event: Option<&str>) -> Option<&'static str> {
    // Subagent events must not mark the enclosing terminal's main turn complete.
    if ["agent_id", "subagent_id", "parent_session_id", "parentSessionId"].iter().any(|k| input.get(k).is_some_and(|v| !v.is_null() && v.as_str() != Some(""))) { return None; }
    let event = override_event.or_else(|| input.get("hook_event_name").and_then(Value::as_str)).or_else(|| input.get("type").and_then(Value::as_str))?;
    match (agent, event) {
        (_, "SubagentStop" | "subagentStop") => None,
        (Agent::Claude | Agent::Codex, "UserPromptSubmit" | "PreToolUse" | "PostToolUse") => Some("working"),
        (Agent::Claude | Agent::Codex, "PermissionRequest") => Some("needs_input"),
        (Agent::Claude | Agent::Codex, "Stop") => Some("completed"),
        (Agent::Codex, "agent-turn-complete") => Some("completed"),
        (Agent::Claude | Agent::Codex, "SessionEnd" | "Interrupt" | "StopFailure") => Some("unknown"),
        (Agent::Claude, "Notification") if input["notification_type"] == "permission_prompt" => Some("needs_input"),
        (Agent::Gemini, "BeforeAgent" | "BeforeTool" | "AfterTool") => Some("working"),
        (Agent::Gemini, "AfterAgent") => Some("completed"),
        (Agent::Gemini, "Notification") if input["notification_type"] == "ToolPermission" => Some("needs_input"),
        (Agent::Gemini, "SessionEnd") => Some("unknown"),
        (Agent::Copilot, "userPromptSubmitted" | "preToolUse" | "postToolUse") => Some("working"),
        (Agent::Copilot, "agentStop") => Some("completed"),
        (Agent::Copilot, "sessionEnd" | "errorOccurred") => Some("unknown"),
        (Agent::Cursor, "beforeSubmitPrompt" | "preToolUse" | "postToolUse") => Some("working"),
        (Agent::Cursor, "stop") => match input["status"].as_str() { Some("completed") => Some("completed"), Some("aborted" | "error") => Some("unknown"), _ => None },
        (Agent::Aider, "completed") => Some("completed"),
        _ => None,
    }
}
fn quote(path: &str) -> anyhow::Result<String> {
    if cfg!(windows) {
        anyhow::ensure!(!path.contains(['"', '%', '!', '\n', '\r']), "Executable path cannot be represented safely in a Windows hook command");
        Ok(format!("\"{}\"", path.replace('\\', "/")))
    } else { Ok(format!("'{}'", path.replace('\'', "'\\''"))) }
}
pub fn hooks(agent: Agent, executable: &str) -> anyhow::Result<Value> {
    if matches!(agent, Agent::Opencode) {
        let script = include_str!("opencode-plugin.js").replace("__GTMUX_EXECUTABLE__", &serde_json::to_string(executable)?);
        return Ok(Value::String(script));
    }
    let command = format!("{} agent event {}", quote(executable)?, name(agent));
    if matches!(agent, Agent::Aider) { return Ok(json!({"notifications":true,"notifications-command":format!("{command} --event completed")})); }
    let events: &[&str] = match agent {
        Agent::Claude => &["UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Notification", "Stop", "SessionEnd"],
        Agent::Codex => &["UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "Interrupt", "SessionEnd"],
        Agent::Gemini => &["BeforeAgent", "BeforeTool", "AfterTool", "AfterAgent", "Notification", "SessionEnd"],
        Agent::Copilot => &["userPromptSubmitted", "preToolUse", "postToolUse", "agentStop", "sessionEnd", "errorOccurred"],
        Agent::Cursor => &["beforeSubmitPrompt", "preToolUse", "postToolUse", "stop"],
        Agent::Aider | Agent::Opencode => unreachable!(),
    };
    let mut result = json!({"hooks":{}});
    if matches!(agent, Agent::Cursor | Agent::Copilot) { result["version"] = json!(1); }
    for event in events {
        result["hooks"][*event] = match agent {
            Agent::Copilot => json!([{"type":"command","exec":executable,"args":["agent","event","copilot","--event",event],"timeoutSec":3}]),
            Agent::Cursor => json!([{"command":format!("{command} --event {event}")}]),
            _ => json!([{"hooks":[{"type":"command","command":command,"timeout":3}]}]),
        };
    }
    Ok(result)
}
pub fn run(command: AgentCmd) -> ExitCode {
    match command {
        AgentCmd::Hooks { agent } => {
            let result = std::env::current_exe().map_err(anyhow::Error::from).and_then(|p| hooks(agent, &p.to_string_lossy()));
            match result { Ok(value) => { if let Some(script) = value.as_str() { print!("{script}"); } else { println!("{}", serde_json::to_string_pretty(&value).unwrap()); } ExitCode::SUCCESS }, Err(e) => { eprintln!("{e}"); ExitCode::FAILURE } }
        }
        AgentCmd::Event { agent, event } => {
            // Never interfere with ordinary agent sessions outside a gtmux pane.
            if let (Ok(target), Ok(instance)) = (std::env::var("GTMUX_TERMINAL_ID"), std::env::var("GTMUX_SERVER_INSTANCE")) {
                let mut data = String::new();
                let read_ok = if matches!(agent, Agent::Aider) { true } else { std::io::stdin().take(1_048_577).read_to_string(&mut data).is_ok() && data.len() <= 1_048_576 };
                if read_ok {
                    let input = if matches!(agent, Agent::Aider) { Some(json!({})) } else { serde_json::from_str(&data).ok() };
                    if let Some(state) = input.as_ref().and_then(|v| state(agent, v, event.as_deref())) {
                        let _ = crate::remote::run_terminal(crate::remote::TerminalCmd::Report { state: state.into(), target, instance: crate::remote::InstanceOpt { instance: Some(instance) } });
                    }
                }
            }
            // Empty JSON is observational for all supported JSON hook protocols.
            if !matches!(agent, Agent::Aider) { println!("{{}}"); }
            ExitCode::SUCCESS
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn main_turn_and_permission_signals() {
        for agent in [Agent::Claude, Agent::Codex] {
            assert_eq!(state(agent, &json!({"hook_event_name":"UserPromptSubmit"}), None), Some("working"));
            assert_eq!(state(agent, &json!({"hook_event_name":"PermissionRequest"}), None), Some("needs_input"));
            assert_eq!(state(agent, &json!({"hook_event_name":"Stop"}), None), Some("completed"));
            assert_eq!(state(agent, &json!({"hook_event_name":"SubagentStop"}), None), None);
            assert_eq!(state(agent, &json!({"hook_event_name":"Stop","agent_id":"child"}), None), None);
        }
    }
    #[test] fn unrelated_notifications_and_errors_do_not_complete_turns() {
        assert_eq!(state(Agent::Claude, &json!({"hook_event_name":"Notification","notification_type":"auth_success"}), None), None);
        assert_eq!(state(Agent::Gemini, &json!({"hook_event_name":"Notification","notification_type":"ToolPermission"}), None), Some("needs_input"));
        assert_eq!(state(Agent::Cursor, &json!({"status":"error"}), Some("stop")), Some("unknown"));
        assert_eq!(state(Agent::Copilot, &json!({}), Some("sessionEnd")), Some("unknown"));
        assert_eq!(state(Agent::Aider, &json!({}), Some("completed")), Some("completed"));
    }
    #[test] fn generated_configs_do_not_change_permissions() {
        for agent in [Agent::Claude, Agent::Codex, Agent::Gemini, Agent::Copilot, Agent::Cursor, Agent::Aider] {
            let config = hooks(agent, "/a path/gtmux").unwrap();
            assert!(!config.to_string().contains("allow"));
            assert!(!config.to_string().contains("SubagentStop"));
        }
        assert_eq!(hooks(Agent::Copilot, "C:/a path/gtmux.exe").unwrap()["hooks"]["agentStop"][0]["exec"], "C:/a path/gtmux.exe");
    }
}
