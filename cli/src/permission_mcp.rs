//! Private stdio MCP server used by Claude Code's `--permission-prompt-tool`.
//!
//! It intentionally exposes one tool and no filesystem/network capabilities.
//! The Claude process supplies only `{tool_name, input}`; the local broker
//! binds that exact input to the owner-scoped relay approval before returning
//! Claude's required JSON-stringified allow/deny payload.

use std::env;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::ApprovalBroker;

const SERVER_NAME: &str = "rebeam_permissions";
const TOOL_NAME: &str = "request";

pub async fn run() -> Result<()> {
    let relay = required_env("REBEAM_RELAY")?;
    let token = required_env("REBEAM_MACHINE_TOKEN")?;
    let agent = required_env("REBEAM_APPROVAL_AGENT")?;
    let chat = required_env("REBEAM_APPROVAL_CHAT")?;
    let provider = env::var("REBEAM_APPROVAL_PROVIDER").unwrap_or_else(|_| "claude".into());
    let project = env::var("REBEAM_APPROVAL_PROJECT").ok();
    let timeout = env::var("REBEAM_APPROVAL_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(15 * 60));

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {token}")
            .parse()
            .context("machine credential is invalid")?,
    );
    let http = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;
    let broker = ApprovalBroker::new(http, relay, agent, chat, provider, project);

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::BufWriter::new(tokio::io::stdout());
    let mut call_number = 0_u64;

    while let Some(line) = lines.next_line().await? {
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(id) = request.get("id").cloned() else {
            // MCP notifications do not receive responses.
            continue;
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") }
                }
            }),
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [{
                    "name": TOOL_NAME,
                    "description": "Ask the Rebeam owner before running one exact Claude tool call.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "tool_name": { "type": "string" },
                            "input": { "type": "object", "additionalProperties": true }
                        },
                        "required": ["tool_name", "input"]
                    }
                }] }
            }),
            "tools/call" => {
                call_number = call_number.saturating_add(1);
                let arguments = request
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let tool = arguments
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned();
                let input = arguments.get("input").cloned().unwrap_or(Value::Null);
                let call_id = format!("mcp_call_{call_number}");
                let target = preview_target(&input);
                let result = match broker
                    .open(&input, &call_id, &tool, target.as_deref(), timeout)
                    .await
                {
                    Ok(pending) => {
                        let cancel = pending.clone();
                        let mut waiting = Box::pin(pending.wait(timeout));
                        let allowed = tokio::select! {
                            result = &mut waiting => result.unwrap_or(false),
                            next = lines.next_line() => {
                                let _ = cancel.cancel("Claude MCP transport closed").await;
                                let _ = next;
                                false
                            }
                        };
                        if allowed {
                            json!({ "behavior": "allow", "updatedInput": input })
                        } else {
                            json!({
                                "behavior": "deny",
                                "message": "The Rebeam owner denied this action or the approval expired."
                            })
                        }
                    }
                    Err(error) => json!({
                        "behavior": "deny",
                        "message": format!("Rebeam approval failed closed: {error:#}")
                    }),
                };
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": serde_json::to_string(&result)? }],
                        "isError": false
                    }
                })
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "method not found" }
            }),
        };
        stdout
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String> {
    let value = env::var(name)
        .with_context(|| format!("{name} is missing from the private MCP environment"))?;
    if value.trim().is_empty() {
        bail!("{name} is empty");
    }
    Ok(value)
}

fn preview_target(input: &Value) -> Option<String> {
    const KEYS: &[&str] = &["command", "file_path", "path", "pattern", "url", "query"];
    match input {
        Value::Object(map) => {
            for key in KEYS {
                if let Some(value) = map.get(*key).and_then(Value::as_str) {
                    return Some(value.to_owned());
                }
            }
            map.values().find_map(preview_target)
        }
        Value::Array(values) => values.iter().find_map(preview_target),
        _ => None,
    }
}
