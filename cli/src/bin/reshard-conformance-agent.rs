//! `reshard-conformance-agent` — a deterministic ACP agent used only by the
//! conformance test (`cli/tests/conformance.rs`).
//!
//! It is NOT a real provider. It implements the agent side of the ACP contract
//! with behavior selected by the prompt text, so one binary exercises every
//! stage reshard depends on: initialize, session/new, streaming, tool telemetry,
//! `session/request_permission`, cancellation, and crash recovery.
//!
//! All per-turn work runs inside `connection.spawn` (holding the responder to
//! answer later). That is the ACP contract's required shape: a handler must not
//! block the message loop, or an in-turn `session/request_permission` round-trip
//! would deadlock against the client's response.
//!
//! Prompt → behavior:
//!   "ping"   → stream one chunk "pong", end the turn
//!   "tool"   → emit a ToolCall update, then stream "ran tool", end the turn
//!   "permit" → request permission from the client, then stream "allowed"/"denied"
//!   "hang"   → never respond (client cancels by abandoning + dropping the conn)
//!   "crash"  → exit the process mid-turn (simulates a provider crash)

use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, InitializeRequest,
    InitializeResponse, NewSessionRequest, NewSessionResponse, PermissionOption,
    PermissionOptionKind, PromptRequest, PromptResponse, RequestPermissionOutcome,
    RequestPermissionRequest, SessionId, SessionNotification, SessionUpdate, StopReason,
    TextContent, ToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Result, Stdio as AcpStdio};

fn prompt_text(req: &PromptRequest) -> String {
    req.prompt
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn stream_text(conn: &ConnectionTo<Client>, session_id: &SessionId, text: &str) -> Result<()> {
    conn.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(TextContent::new(
            text,
        )))),
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    Agent
        .builder()
        .name("reshard-conformance-agent")
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
            async |_req: NewSessionRequest, responder, _connection| {
                let id = format!("conf-{}", uuid::Uuid::new_v4().simple());
                responder.respond(NewSessionResponse::new(SessionId::new(id)))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            // A conformant agent must accept session/cancel without erroring.
            async |_note: CancelNotification, _cx| Ok(()),
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async |req: PromptRequest, responder, connection| {
                let conn = connection.clone();
                connection.spawn(async move {
                    let session_id = req.session_id.clone();
                    match prompt_text(&req).trim() {
                        "ping" => {
                            stream_text(&conn, &session_id, "pong").await?;
                            responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                        }
                        "tool" => {
                            conn.send_notification(SessionNotification::new(
                                session_id.clone(),
                                SessionUpdate::ToolCall(ToolCall::new(
                                    ToolCallId::new("call_1"),
                                    "Bash: echo hi",
                                )),
                            ))?;
                            stream_text(&conn, &session_id, "ran tool").await?;
                            responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                        }
                        "permit" => {
                            let outcome = conn
                                .send_request(RequestPermissionRequest::new(
                                    session_id.clone(),
                                    ToolCallUpdate::new(
                                        ToolCallId::new("call_1"),
                                        ToolCallUpdateFields::new()
                                            .title("Bash: rm test.txt")
                                            .kind(ToolKind::Execute)
                                            .status(ToolCallStatus::Pending),
                                    ),
                                    vec![
                                        PermissionOption::new(
                                            "allow_once",
                                            "Allow once",
                                            PermissionOptionKind::AllowOnce,
                                        ),
                                        PermissionOption::new(
                                            "reject_once",
                                            "Reject once",
                                            PermissionOptionKind::RejectOnce,
                                        ),
                                    ],
                                ))
                                .block_task()
                                .await?;
                            let allowed = matches!(
                                outcome.outcome,
                                RequestPermissionOutcome::Selected(ref sel)
                                    if sel.option_id.0.as_ref() == "allow_once"
                            );
                            stream_text(
                                &conn,
                                &session_id,
                                if allowed { "allowed" } else { "denied" },
                            )
                            .await?;
                            responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                        }
                        "hang" => {
                            // Never respond: the client cancels by abandoning the
                            // prompt and dropping the connection (reshard's real path).
                            tokio::time::sleep(Duration::from_secs(3600)).await;
                            responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                        }
                        "crash" => {
                            std::process::exit(1);
                        }
                        _ => {
                            stream_text(&conn, &session_id, "unknown").await?;
                            responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                        }
                    }
                    Ok(())
                })?;
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(AcpStdio::new())
        .await
}
