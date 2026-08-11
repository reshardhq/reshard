//! `claude-code-acp` — rebeam's own ACP adapter for the Claude Code CLI.
//!
//! rebeam ships this so `--provider claude` needs **no Node** and uses the
//! user's **Claude Code subscription login**: an ACP client (the rebeam gateway)
//! spawns this over stdio, and it drives `claude -p --output-format stream-json`,
//! translating Claude's JSONL into ACP session updates and permission requests.
//!
//! Subscription auth: we strip `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` from
//! the child's env so `claude` authenticates via `claude login` (the Agent SDK
//! path refuses subscription auth — that's why we wrap the CLI, not the SDK).
//!
//! Design ported from harukitosa/claude-code-acp (MIT). Streaming, tool
//! telemetry, resume, and the private MCP permission bridge are kept in this
//! small provider shim; subscription credentials never enter the SDK path.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest,
    PromptResponse, SessionId, SessionNotification, SessionUpdate, StopReason, TextContent,
    ToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Agent, Result, Stdio as AcpStdio};
use anyhow::{Context, Result as AnyhowResult};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

/// ACP session id → what we need to drive and resume the matching Claude run.
#[derive(Default, Clone)]
struct SessionState {
    cwd: Option<PathBuf>,
    claude_session_id: Option<String>,
}

type Sessions = Arc<Mutex<HashMap<String, SessionState>>>;

/// Where per-chat Claude session ids are persisted. The gateway spawns this
/// adapter fresh for every message, so in-memory state can't carry a
/// conversation forward — we persist Claude's `session_id` keyed by chat
/// (REBEAM_CHAT) and `--resume` it, which is what makes the agent remember.
fn session_file(chat: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let safe: String = chat
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    Some(
        PathBuf::from(home)
            .join(".rebeam")
            .join("sessions")
            .join(format!("{safe}.txt")),
    )
}

fn load_claude_session(chat: &str) -> Option<String> {
    let text = std::fs::read_to_string(session_file(chat)?).ok()?;
    let trimmed = text.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn save_claude_session(chat: &str, id: &str) {
    if let Some(path) = session_file(chat) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, id);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let for_new = sessions.clone();
    let for_prompt = sessions.clone();

    Agent
        .builder()
        .name("claude-code-acp")
        .on_receive_request(
            async |req: InitializeRequest, responder, _connection| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, _connection| {
                let id = format!("sess-{}", uuid::Uuid::new_v4().simple());
                for_new.lock().await.insert(
                    id.clone(),
                    SessionState {
                        cwd: Some(req.cwd.clone()),
                        claude_session_id: None,
                    },
                );
                responder.respond(NewSessionResponse::new(SessionId::new(id)))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, connection| {
                let key = req.session_id.0.to_string();
                let (cwd, claude_session_id) = {
                    let map = for_prompt.lock().await;
                    let state = map.get(&key).cloned().unwrap_or_default();
                    (state.cwd, state.claude_session_id)
                };

                // Flatten the prompt's text blocks into the argument Claude reads.
                let text = req
                    .prompt
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let mut args = vec![
                    "-p".to_string(),
                    text,
                    "--output-format".into(),
                    "stream-json".into(),
                    "--verbose".into(),
                    // Don't inherit the user's personal MCP servers (reminders,
                    // calendar, contacts, …). They trigger macOS permission
                    // prompts attributed to `rebeam` and aren't the agent's job.
                    "--strict-mcp-config".into(),
                    // Keep Claude's normal permission boundary. Gateway mode
                    // adds the private permission-prompt MCP bridge below;
                    // terminal mode has no remote approval authority.
                    "--permission-mode".into(),
                    "default".into(),
                ];
                let mcp_config = if std::env::var_os("REBEAM_MACHINE_TOKEN").is_some() {
                    let config = write_permission_mcp_config()?;
                    args.extend([
                        "--mcp-config".into(),
                        config.display().to_string(),
                        "--permission-prompt-tool".into(),
                        "mcp__rebeam_permissions__request".into(),
                        "--allowedTools".into(),
                        "mcp__rebeam_permissions__request".into(),
                    ]);
                    Some(config)
                } else {
                    None
                };
                // Resume this chat's Claude session so the agent remembers the
                // conversation. Persisted per chat (REBEAM_CHAT) because the
                // gateway spawns this adapter fresh on every message.
                let resume_id = std::env::var("REBEAM_CHAT")
                    .ok()
                    .filter(|c| !c.is_empty())
                    .and_then(|c| load_claude_session(&c))
                    .or_else(|| claude_session_id.clone());
                if let Some(sid) = &resume_id {
                    args.push("--resume".into());
                    args.push(sid.clone());
                }

                let mut command = Command::new("claude");
                command
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    // Force subscription auth — no API-key fallback.
                    .env_remove("ANTHROPIC_API_KEY")
                    .env_remove("ANTHROPIC_AUTH_TOKEN")
                    // The machine credential is for the private MCP child,
                    // never for Claude itself or a provider tool subprocess.
                    .env_remove("REBEAM_MACHINE_TOKEN");
                if let Some(cwd) = &cwd {
                    command.current_dir(cwd);
                }

                let mut child = match command.spawn() {
                    Ok(child) => child,
                    Err(err) => {
                        if let Some(config) = &mcp_config {
                            let _ = std::fs::remove_file(config);
                        }
                        connection.send_notification(SessionNotification::new(
                            req.session_id.clone(),
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                                TextContent::new(format!("failed to launch `claude`: {err}")),
                            ))),
                        ))?;
                        return responder.respond(PromptResponse::new(StopReason::EndTurn));
                    }
                };

                let stdout = child.stdout.take().expect("stdout piped");
                let mut lines = BufReader::new(stdout).lines();
                let mut tool_counter: u32 = 0;
                let mut new_claude_session: Option<String> = None;

                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(value) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if let Some(sid) = value.get("session_id").and_then(Value::as_str) {
                        new_claude_session = Some(sid.to_string());
                    }
                    match value.get("type").and_then(Value::as_str) {
                        Some("assistant") => {
                            if let Some(content) = value
                                .get("message")
                                .and_then(|m| m.get("content"))
                                .and_then(Value::as_array)
                            {
                                for block in content {
                                    match block.get("type").and_then(Value::as_str) {
                                        Some("text") => {
                                            if let Some(t) =
                                                block.get("text").and_then(Value::as_str)
                                            {
                                                connection.send_notification(
                                                    SessionNotification::new(
                                                        req.session_id.clone(),
                                                        SessionUpdate::AgentMessageChunk(
                                                            ContentChunk::new(ContentBlock::Text(
                                                                TextContent::new(t),
                                                            )),
                                                        ),
                                                    ),
                                                )?;
                                            }
                                        }
                                        Some("tool_use") => {
                                            tool_counter += 1;
                                            let name = block
                                                .get("name")
                                                .and_then(Value::as_str)
                                                .unwrap_or("tool");
                                            // A short preview from whatever the
                                            // tool acted on, for the chip title.
                                            let preview = block
                                                .get("input")
                                                .and_then(|input| {
                                                    ["command", "file_path", "path", "pattern", "url"]
                                                        .iter()
                                                        .find_map(|k| {
                                                            input.get(k).and_then(Value::as_str)
                                                        })
                                                })
                                                .unwrap_or("");
                                            let title = if preview.is_empty() {
                                                name.to_string()
                                            } else {
                                                format!("{name}: {preview}")
                                            };
                                            connection.send_notification(
                                                SessionNotification::new(
                                                    req.session_id.clone(),
                                                    SessionUpdate::ToolCall(ToolCall::new(
                                                        ToolCallId::new(format!(
                                                            "call_{tool_counter}"
                                                        )),
                                                        title,
                                                    )),
                                                ),
                                            )?;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        // Preserve legacy permission events for ACP clients that
                        // expose them directly. Claude's print-mode enforcement
                        // uses the private MCP permission tool above.
                        Some("permission_request") => {
                            tool_counter += 1;
                            let name = value
                                .get("tool_name")
                                .or_else(|| value.get("toolName"))
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_string();
                            let _outcome = connection
                                .send_request(agent_client_protocol::schema::v1::RequestPermissionRequest::new(
                                    req.session_id.clone(),
                                    ToolCallUpdate::new(
                                        ToolCallId::new(format!("call_{tool_counter}")),
                                        ToolCallUpdateFields::new()
                                            .title(name)
                                            .kind(ToolKind::Execute)
                                            .status(ToolCallStatus::Pending),
                                    ),
                                    vec![
                                        PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
                                        PermissionOption::new("allow_always", "Allow always", PermissionOptionKind::AllowAlways),
                                        PermissionOption::new("reject_once", "Reject once", PermissionOptionKind::RejectOnce),
                                        PermissionOption::new("reject_always", "Reject always", PermissionOptionKind::RejectAlways),
                                    ],
                                ))
                                .block_task()
                                .await?;
                        }
                        Some("result") => break,
                        _ => {}
                    }
                }

                if let Some(config) = mcp_config {
                    let _ = std::fs::remove_file(config);
                }

                if let Some(sid) = new_claude_session {
                    // Persist for the next message in this chat (continuity).
                    if let Ok(chat) = std::env::var("REBEAM_CHAT") {
                        if !chat.is_empty() {
                            save_claude_session(&chat, &sid);
                        }
                    }
                    for_prompt
                        .lock()
                        .await
                        .entry(key)
                        .or_default()
                        .claude_session_id = Some(sid);
                }

                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(AcpStdio::new())
        .await
}

fn write_permission_mcp_config() -> AnyhowResult<std::path::PathBuf> {
    let executable =
        rebeam_cli_path().context("the rebeam CLI is not available next to claude-code-acp")?;
    let root = std::env::var_os("REBEAM_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".rebeam"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let dir = root.join("runs");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating private MCP directory {}", dir.display()))?;
    let path = dir.join(format!("permission-{}.json", uuid::Uuid::new_v4().simple()));
    let config = serde_json::json!({
        "mcpServers": {
            "rebeam_permissions": {
                "command": executable,
                "args": ["permission-mcp"],
                "env": {
                    "REBEAM_MACHINE_TOKEN": std::env::var("REBEAM_MACHINE_TOKEN")
                        .context("machine credential is missing from the adapter")?
                }
            }
        }
    });
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&config)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&temporary, &path)?;
    Ok(path)
}

fn rebeam_cli_path() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("REBEAM_CLI").map(std::path::PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))?;
    for name in ["rebeam", "rebeam.exe"] {
        let candidate = sibling.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join("rebeam"))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_config_is_private_and_contains_only_the_mcp_child_secret() {
        let root = std::env::temp_dir().join(format!(
            "rebeam-mcp-config-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let cli = root.join("rebeam");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&cli, "fake cli").unwrap();
        std::env::set_var("REBEAM_HOME", &root);
        std::env::set_var("REBEAM_CLI", &cli);
        std::env::set_var("REBEAM_MACHINE_TOKEN", "rm_ephemeral_test");

        let path = write_permission_mcp_config().unwrap();
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcpServers"]["rebeam_permissions"]["args"][0],
            "permission-mcp"
        );
        assert_eq!(
            config["mcpServers"]["rebeam_permissions"]["env"]["REBEAM_MACHINE_TOKEN"],
            "rm_ephemeral_test"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(root);
        std::env::remove_var("REBEAM_HOME");
        std::env::remove_var("REBEAM_CLI");
        std::env::remove_var("REBEAM_MACHINE_TOKEN");
    }
}
