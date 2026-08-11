//! `rebeam acp` — the Agent Client Protocol bridge.
//!
//! Any ACP-speaking agent (our `claude-code-acp`, `codex-acp`, Gemini CLI's
//! `--experimental-acp`) is invoked the same way: we play the *client* role,
//! spawn the agent as a subprocess over stdio, and translate its session
//! events into Rebeam's model. Gateway prompts reuse one worker per chat;
//! terminal probes remain one-shot.
//!
//! This is what makes the bridge agent-agnostic. Instead of a bespoke parser
//! per provider, one ACP client handles them all:
//!
//! - `session/update` → streaming text + tool-call telemetry (Rebeam `Status`).
//! - `session/request_permission` → a local terminal prompt or an owner-only
//!   Rebeam approval card. Permission is denied unless one exact invocation is
//!   explicitly released.
//!
//! Two modes: `Terminal` (the interactive `rebeam acp` probe) and `Gateway`
//! (driven by `rebeam gateway`: posts tool telemetry to the relay as `Status`
//! and prints the final reply on stdout for the gateway to `Send`).

use std::collections::HashMap;
use std::io::Write;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, LoadSessionRequest, NewSessionRequest,
    PermissionOptionKind, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use anyhow::{anyhow, bail, Result};
use owo_colors::OwoColorize;
use rebeam_core::{Command as Cmd, StatusState};
use tokio::sync::{mpsc, oneshot, Mutex};

/// Resolve a provider name to the command that launches its ACP adapter, with
/// no manual adapter install. `rebeam acp --command "<launcher>"` overrides.
pub fn adapter_command(provider: &str) -> Result<String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        // Our own CLI-driven adapter: uses the Claude Code subscription login,
        // no Node, no API key. This is the default because it's what users have.
        "claude" => claude_code_adapter(),
        // The official Agent-SDK adapter — API-key billing only (Anthropic
        // blocks subscription auth for the SDK). Opt in with `--provider claude-api`.
        "claude-api" => npx("@agentclientprotocol/claude-agent-acp"),
        "codex" => npx("@zed-industries/codex-acp"),
        // The Gemini CLI is an ACP agent itself — no adapter needed.
        "gemini" => native("gemini --experimental-acp", "gemini"),
        other => bail!(
            "unknown provider {other:?}. Known: claude, claude-api, codex, gemini. \
             For anything else pass --command \"<acp launcher>\"."
        ),
    }
}

/// The `claude-code-acp` binary rebeam ships alongside itself — prefer the one
/// next to the running `rebeam`, else fall back to PATH.
fn claude_code_adapter() -> Result<String> {
    // A Claude subscription adapter may be installed by `rebeam setup` as a
    // managed sidecar. Keep this override explicit so development checkouts
    // and packaged desktop builds can point at a pinned adapter bundle.
    if let Some(configured) = std::env::var_os("REBEAM_CLAUDE_ACP") {
        let path = std::path::PathBuf::from(configured);
        if path.is_file() {
            return Ok(path.display().to_string());
        }
        bail!(
            "REBEAM_CLAUDE_ACP points to a missing adapter: {}",
            path.display()
        );
    }

    // First prefer the managed subscription sidecar. It is intentionally
    // separate from the Rebeam binary so the Python runtime can be replaced
    // or converted to Rust later without changing the CLI contract.
    if let Some(home) = std::env::var_os("REBEAM_HOME") {
        let managed = std::path::PathBuf::from(home)
            .join("runtimes")
            .join("claude-subscription")
            .join("venv")
            .join("bin")
            .join("claude-code-acp");
        if managed.is_file() {
            return Ok(managed.display().to_string());
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let managed = std::path::PathBuf::from(home)
            .join(".rebeam")
            .join("runtimes")
            .join("claude-subscription")
            .join("venv")
            .join("bin")
            .join("claude-code-acp");
        if managed.is_file() {
            return Ok(managed.display().to_string());
        }
    }

    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("claude-code-acp")))
        .filter(|path| path.is_file());
    let command = match sibling {
        Some(path) => path.display().to_string(),
        None if on_path("claude-code-acp") => "claude-code-acp".to_string(),
        None => bail!("the claude-code-acp adapter is missing (not next to rebeam or on PATH)"),
    };
    if !on_path("claude") {
        eprintln!(
            "{} the `claude` CLI is not on PATH — the adapter needs it; run `claude login`.",
            "warning".yellow()
        );
    }
    Ok(command)
}

/// An npm-published adapter, launched on demand. `npx -y` runs it without a
/// global install and caches it after the first fetch.
fn npx(package: &str) -> Result<String> {
    if !on_path("npx") {
        bail!(
            "`npx` (Node.js) is needed to launch the {package} adapter.\n  \
             Install Node 18+, or pass --command \"<acp launcher>\" to point at your own."
        );
    }
    Ok(format!("npx -y {package}"))
}

/// An agent CLI that speaks ACP natively — we just add its flag.
fn native(command: &str, bin: &str) -> Result<String> {
    if !on_path(bin) {
        bail!("`{bin}` is not installed or not on PATH.");
    }
    Ok(command.to_string())
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(bin);
                candidate.is_file()
                    || candidate.with_extension("exe").is_file()
                    || candidate.with_extension("cmd").is_file()
            })
        })
        .unwrap_or(false)
}

/// How permission requests are answered. Asking is the default.
#[derive(Clone, Copy)]
pub enum Approve {
    /// Ask locally in terminal mode, or through the owner card in gateway mode.
    Ask,
    /// Grant every request (pick an allow-* option).
    Auto,
    /// Refuse every request (pick a reject-* option, else cancel).
    Deny,
}

/// How a session is driven and where its events go.
pub enum Mode {
    /// Stream to the terminal (the `rebeam acp` probe).
    Terminal,
    /// Drive from the gateway: post `Status` per tool call to the relay, and
    /// print the final reply to stdout for the gateway to `Send`.
    Gateway {
        relay: String,
        chat: String,
        agent: String,
        token: String,
        provider: String,
    },
}

/// Terminal, or a relay-posting sink with an accumulating reply buffer.
#[derive(Clone)]
enum Sink {
    Terminal,
    Gateway(Arc<GatewaySink>),
}

struct GatewaySink {
    http: reqwest::Client,
    relay: String,
    chat: String,
    agent: String,
    text: Mutex<String>,
    approvals: crate::approval::ApprovalBroker,
}

struct GatewayWorkerRequest {
    prompt: String,
    sink: Sink,
    reply: oneshot::Sender<Result<RunOutcome, String>>,
}

#[derive(Debug)]
pub struct RunOutcome {
    pub text: String,
    pub session_id: Option<String>,
    pub resumed: bool,
}

static GATEWAY_WORKERS: OnceLock<Mutex<HashMap<String, mpsc::Sender<GatewayWorkerRequest>>>> =
    OnceLock::new();
static WORKER_CIRCUITS: OnceLock<Mutex<HashMap<String, WorkerCircuit>>> = OnceLock::new();
static WORKER_CANCELLATIONS: OnceLock<Mutex<HashMap<String, mpsc::Sender<()>>>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
struct WorkerCircuit {
    failures: u32,
    retry_at: Instant,
}

const CIRCUIT_THRESHOLD: u32 = 5;
const CIRCUIT_OPEN_FOR: Duration = Duration::from_secs(5 * 60);

fn worker_retry_delay(failures: u32) -> Duration {
    if failures >= CIRCUIT_THRESHOLD {
        CIRCUIT_OPEN_FOR
    } else {
        Duration::from_secs(1_u64 << failures.saturating_sub(1).min(6))
    }
}

pub async fn cancel_gateway_session(agent: &str, chat: &str) {
    let prefix = format!("{agent}:{chat}:");
    let Some(cancellations) = WORKER_CANCELLATIONS.get() else {
        return;
    };
    let senders: Vec<_> = cancellations
        .lock()
        .await
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, sender)| sender.clone())
        .collect();
    for sender in senders {
        let _ = sender.send(()).await;
    }
}

pub async fn cancel_all_gateway_sessions() {
    let Some(cancellations) = WORKER_CANCELLATIONS.get() else {
        return;
    };
    let senders: Vec<_> = cancellations.lock().await.values().cloned().collect();
    for sender in senders {
        let _ = sender.send(()).await;
    }
}

/// Spawn or reuse an ACP agent, run one prompt, and route its events per `mode`.
///
/// `cwd` is the agent's working directory — the project it operates in, so
/// `claude` reads that project's `CLAUDE.md`/docs. Defaults to the process cwd.
pub async fn run(
    command: &str,
    prompt: &str,
    approve: Approve,
    mode: Mode,
    cwd: Option<std::path::PathBuf>,
    resume_session: Option<String>,
) -> Result<RunOutcome> {
    let agent =
        AcpAgent::from_str(command).map_err(|e| anyhow!("cannot launch agent {command:?}: {e}"))?;
    let prompt = prompt.to_string();
    let session_cwd = cwd.unwrap_or_else(default_cwd);

    let sink = match mode {
        Mode::Terminal => {
            eprintln!("{} {}", "acp".green().bold(), command.dimmed());
            Sink::Terminal
        }
        Mode::Gateway {
            relay,
            chat,
            agent: author,
            token,
            provider,
        } => {
            // The private MCP child inherits these short-lived values from
            // this gateway process. They never go into the MCP config file.
            std::env::set_var("REBEAM_RELAY", &relay);
            std::env::set_var("REBEAM_MACHINE_TOKEN", &token);
            std::env::set_var("REBEAM_APPROVAL_AGENT", &author);
            std::env::set_var("REBEAM_APPROVAL_CHAT", &chat);
            std::env::set_var("REBEAM_APPROVAL_PROVIDER", &provider);
            std::env::set_var("REBEAM_APPROVAL_PROJECT", session_cwd.display().to_string());
            let mut headers = reqwest::header::HeaderMap::new();
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|_| anyhow!("invalid machine token"))?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
            let http = reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .map_err(|e| anyhow!("http client: {e}"))?;
            Sink::Gateway(Arc::new(GatewaySink {
                approvals: crate::approval::ApprovalBroker::new(
                    http.clone(),
                    relay.clone(),
                    author.clone(),
                    chat.clone(),
                    provider,
                    Some(session_cwd.display().to_string()),
                ),
                http,
                relay,
                chat,
                agent: author,
                text: Mutex::new(String::new()),
            }))
        }
    };

    // Gateway turns share a live ACP session. Terminal probes remain one-shot
    // so an interactive command exits naturally after its prompt completes.
    if let Sink::Gateway(gateway) = &sink {
        let key = format!("{}:{}:{}", gateway.agent, gateway.chat, command);
        let (reply_tx, reply_rx) = oneshot::channel();
        let sender = gateway_worker_sender(
            &key,
            command,
            approve,
            session_cwd.clone(),
            resume_session,
            sink.clone(),
        )
        .await?;
        sender
            .send(GatewayWorkerRequest {
                prompt,
                sink: sink.clone(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow!("gateway ACP worker stopped"))?;
        let outcome = reply_rx
            .await
            .map_err(|_| anyhow!("gateway ACP worker dropped the prompt"))?
            .map_err(|error| anyhow!("gateway ACP worker: {error}"))?;
        return Ok(outcome);
    }

    let note_sink = sink.clone();
    let end_sink = sink.clone();
    let permission_sink = sink.clone();

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                handle_update(&note_sink, notification.update).await;
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                let outcome = decide(&request, approve, &permission_sink).await;
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(session_cwd))
                .block_task()
                .await?;
            connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("acp session failed: {e}"))?;

    // Gateway mode returns the whole reply at once, on stdout, for the gateway
    // to post as a message. Terminal mode already streamed it live.
    if let Sink::Gateway(gateway) = &end_sink {
        print!("{}", gateway.text.lock().await);
        let _ = std::io::stdout().flush();
    }

    Ok(RunOutcome {
        text: String::new(),
        session_id: None,
        resumed: false,
    })
}

async fn gateway_worker_sender(
    key: &str,
    command: &str,
    approve: Approve,
    cwd: std::path::PathBuf,
    resume_session: Option<String>,
    initial_sink: Sink,
) -> Result<mpsc::Sender<GatewayWorkerRequest>> {
    let circuits = WORKER_CIRCUITS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(circuit) = circuits.lock().await.get(key).copied() {
        let now = Instant::now();
        if circuit.retry_at > now {
            let wait = circuit.retry_at.duration_since(now).as_secs().max(1);
            let state = if circuit.failures >= CIRCUIT_THRESHOLD {
                "circuit open"
            } else {
                "restart backoff"
            };
            bail!(
                "ACP worker {state} after {} failures; retry in {wait}s",
                circuit.failures
            );
        }
    }
    let workers = GATEWAY_WORKERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = workers.lock().await;
    if let Some(sender) = guard.get(key) {
        return Ok(sender.clone());
    }

    let (tx, mut rx) = mpsc::channel::<GatewayWorkerRequest>(8);
    let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
    WORKER_CANCELLATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .await
        .insert(key.to_string(), cancel_tx);
    let worker_key = key.to_string();
    let success_key = worker_key.clone();
    let command = command.to_string();
    let active_sink = Arc::new(Mutex::new(Some(initial_sink)));
    let notification_sink = active_sink.clone();
    let permission_sink = active_sink.clone();
    // Notifications (streamed text) and permission prompts route through
    // `active_sink`. Each turn brings a fresh per-request sink, so the worker
    // must re-point `active_sink` at it before prompting — otherwise turn 2's
    // output keeps landing in turn 1's dead sink and the reply comes back empty.
    let loop_sink = active_sink.clone();
    tokio::spawn(async move {
        let result = async {
            let agent = AcpAgent::from_str(&command)
                .map_err(|e| anyhow!("cannot launch agent {command:?}: {e}"))?;
            agent_client_protocol::Client
                .builder()
                .on_receive_notification(
                    async move |notification: SessionNotification, _cx| {
                        if let Some(sink) = notification_sink.lock().await.clone() {
                            handle_update(&sink, notification.update).await;
                        }
                        Ok(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .on_receive_request(
                    async move |request: RequestPermissionRequest, responder, _connection| {
                        let outcome = if let Some(sink) = permission_sink.lock().await.clone() {
                            decide(&request, approve, &sink).await
                        } else {
                            RequestPermissionOutcome::Cancelled
                        };
                        responder.respond(RequestPermissionResponse::new(outcome))
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
                    let initialized = connection
                        .send_request(InitializeRequest::new(ProtocolVersion::V1))
                        .block_task()
                        .await?;
                    let can_resume = initialized.agent_capabilities.load_session;
                    let mut resumed = false;
                    let session_id = if let Some(saved) = resume_session.filter(|_| can_resume) {
                        let loaded = connection
                            .send_request(LoadSessionRequest::new(saved.clone(), cwd.clone()))
                            .block_task()
                            .await;
                        if loaded.is_ok() {
                            resumed = true;
                            saved.into()
                        } else {
                            connection
                                .send_request(NewSessionRequest::new(cwd.clone()))
                                .block_task()
                                .await?
                                .session_id
                        }
                    } else {
                        connection
                            .send_request(NewSessionRequest::new(cwd))
                            .block_task()
                            .await?
                            .session_id
                    };
                    while let Some(request) = rx.recv().await {
                        // Route this turn's streamed text/permission prompts to
                        // the current request's sink (see `loop_sink` above).
                        *loop_sink.lock().await = Some(request.sink.clone());
                        if let Sink::Gateway(gateway) = &request.sink {
                            gateway.text.lock().await.clear();
                        }
                        // A provider may finish a tool call but fail to emit
                        // the final prompt response. Bound each turn so one
                        // broken adapter cannot leave the chat permanently
                        // stuck in "working".
                        let prompt = tokio::time::timeout(
                            Duration::from_secs(120),
                            connection
                                .send_request(PromptRequest::new(
                                    session_id.clone(),
                                    vec![ContentBlock::Text(TextContent::new(request.prompt))],
                                ))
                                .block_task(),
                        );
                        let response = tokio::select! {
                            response = prompt => response
                                .map_err(|_| anyhow!("ACP prompt timed out after 120 seconds"))?
                                .map_err(|e| anyhow!("ACP prompt failed: {e}")),
                            _ = cancel_rx.recv() => {
                                connection.send_notification(CancelNotification::new(session_id.clone()))?;
                                Err(anyhow!("ACP prompt cancelled"))
                            }
                        };
                        let response = match response {
                            Ok(_) => {
                                if let Some(circuits) = WORKER_CIRCUITS.get() {
                                    circuits.lock().await.remove(&success_key);
                                }
                                if let Sink::Gateway(gateway) = &request.sink {
                                    Ok(RunOutcome {
                                        text: gateway.text.lock().await.clone(),
                                        session_id: Some(session_id.to_string()),
                                        resumed,
                                    })
                                } else {
                                    Ok(RunOutcome {
                                        text: String::new(),
                                        session_id: Some(session_id.to_string()),
                                        resumed,
                                    })
                                }
                            }
                            Err(error) => Err(error),
                        };
                        let _ = request.reply.send(response.map_err(|e| e.to_string()));
                    }
                    Ok(())
                })
                .await
                .map_err(|e| anyhow!("ACP worker failed: {e}"))?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            eprintln!("{} gateway worker {worker_key}: {error:#}", "error".red());
            if !error.to_string().contains("ACP prompt cancelled") {
                let circuits = WORKER_CIRCUITS.get_or_init(|| Mutex::new(HashMap::new()));
                let mut circuits = circuits.lock().await;
                let failures = circuits
                    .get(&worker_key)
                    .map_or(1, |previous| previous.failures.saturating_add(1));
                let delay = worker_retry_delay(failures);
                circuits.insert(
                    worker_key.clone(),
                    WorkerCircuit {
                        failures,
                        retry_at: Instant::now() + delay,
                    },
                );
                eprintln!(
                    "{} {worker_key} restart delayed {}s after {failures} failures",
                    "backoff".yellow(),
                    delay.as_secs()
                );
            }
        }
        if let Some(workers) = GATEWAY_WORKERS.get() {
            workers.lock().await.remove(&worker_key);
        }
        if let Some(cancellations) = WORKER_CANCELLATIONS.get() {
            cancellations.lock().await.remove(&worker_key);
        }
    });
    guard.insert(key.to_string(), tx.clone());
    Ok(tx)
}

/// Route one streamed event to the terminal or the relay.
async fn handle_update(sink: &Sink, update: SessionUpdate) {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            let text = content_to_string(&chunk.content);
            match sink {
                Sink::Terminal => {
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                Sink::Gateway(gateway) => gateway.text.lock().await.push_str(&text),
            }
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            if let Sink::Terminal = sink {
                eprint!("{}", content_to_string(&chunk.content).dimmed());
            }
        }
        SessionUpdate::ToolCall(call) => match sink {
            Sink::Terminal => eprintln!(
                "{} {} {}",
                "⚙".yellow(),
                call.title.bold(),
                format!("{:?}·{:?}", call.kind, call.status)
                    .to_lowercase()
                    .dimmed(),
            ),
            // Live tool chip in the app: broadcast an (ephemeral) Status event.
            Sink::Gateway(gateway) => {
                let (tool, target) = split_title(&call.title);
                let command = Cmd::Status {
                    chat: gateway.chat.clone(),
                    author: gateway.agent.clone(),
                    state: StatusState::Tool,
                    label: None,
                    tool: Some(tool),
                    target,
                };
                let _ = gateway
                    .http
                    .post(format!("{}/commands", gateway.relay))
                    .json(&command)
                    .send()
                    .await;
            }
        },
        _ => {}
    }
}

/// `"Bash: ls -1"` → `("Bash", Some("ls -1"))`; `"Read"` → `("Read", None)`.
fn split_title(title: &str) -> (String, Option<String>) {
    match title.split_once(": ") {
        Some((tool, target)) => (tool.to_string(), Some(target.to_string())),
        None => (title.to_string(), None),
    }
}

/// Answer a permission request. Gateway asks are routed through the relay;
/// terminal asks require an explicit `y`, with Enter meaning deny.
async fn decide(
    request: &RequestPermissionRequest,
    approve: Approve,
    sink: &Sink,
) -> RequestPermissionOutcome {
    let want_allow = match approve {
        Approve::Auto => true,
        Approve::Deny => false,
        Approve::Ask => match sink {
            Sink::Terminal => terminal_approval().await,
            Sink::Gateway(gateway) => {
                let exact = match serde_json::to_value(request) {
                    Ok(value) => value,
                    Err(error) => {
                        eprintln!(
                            "{} could not encode permission request: {error}",
                            "denied".red()
                        );
                        return RequestPermissionOutcome::Cancelled;
                    }
                };
                let (tool_call, tool, target) = crate::approval::request_metadata(&exact);
                match gateway
                    .approvals
                    .authorize(
                        &exact,
                        &tool_call,
                        &tool,
                        target.as_deref(),
                        Duration::from_secs(15 * 60),
                    )
                    .await
                {
                    Ok(allowed) => allowed,
                    Err(error) => {
                        eprintln!("{} approval failed closed: {error:#}", "denied".red());
                        false
                    }
                }
            }
        },
    };
    let pick = request.options.iter().find(|option| {
        if want_allow {
            option.kind == PermissionOptionKind::AllowOnce
        } else {
            !is_allow(option.kind)
        }
    });
    match pick {
        Some(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            option.option_id.clone(),
        )),
        None => RequestPermissionOutcome::Cancelled,
    }
}

async fn terminal_approval() -> bool {
    eprint!(
        "{} Allow this tool once? [y/N] ",
        "approval".yellow().bold()
    );
    let _ = std::io::stderr().flush();
    tokio::task::spawn_blocking(|| {
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .ok()
            .is_some_and(|_| matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
    })
    .await
    .unwrap_or(false)
}

fn is_allow(kind: PermissionOptionKind) -> bool {
    matches!(
        kind,
        PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
    )
}

fn content_to_string(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(image) => format!("[image: {}]", image.mime_type),
        ContentBlock::Audio(audio) => format!("[audio: {}]", audio.mime_type),
        ContentBlock::ResourceLink(link) => link.uri.clone(),
        _ => String::new(),
    }
}

fn default_cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_failures_back_off_then_open_the_circuit() {
        assert_eq!(worker_retry_delay(1), Duration::from_secs(1));
        assert_eq!(worker_retry_delay(2), Duration::from_secs(2));
        assert_eq!(worker_retry_delay(3), Duration::from_secs(4));
        assert_eq!(worker_retry_delay(4), Duration::from_secs(8));
        assert_eq!(worker_retry_delay(5), CIRCUIT_OPEN_FOR);
        assert_eq!(worker_retry_delay(20), CIRCUIT_OPEN_FOR);
    }
}
