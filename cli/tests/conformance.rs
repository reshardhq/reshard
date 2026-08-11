//! ACP provider conformance contract (Phase 6).
//!
//! Drives `reshard-conformance-agent` through every stage reshard's gateway
//! depends on. Any ACP adapter reshard trusts under the default Ask policy must
//! satisfy this same contract: initialize, session/new, streaming, tool
//! telemetry, `session/request_permission`, cancellation, and crash recovery.

use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use tokio::sync::Mutex;

const AGENT: &str = env!("CARGO_BIN_EXE_reshard-conformance-agent");

/// What the client observed the agent stream during a turn.
#[derive(Default)]
struct Recorder {
    text: Mutex<String>,
    tools: Mutex<Vec<String>>,
    perms: AtomicUsize,
}

fn text_block(s: &str) -> ContentBlock {
    ContentBlock::Text(TextContent::new(s))
}

fn cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap()
}

/// initialize → session → streaming → tool telemetry → permission → cancellation,
/// all against one long-lived provider connection.
#[tokio::test]
async fn conformance_full_contract() {
    let rec = Arc::new(Recorder::default());
    let note_rec = rec.clone();
    let perm_rec = rec.clone();
    let run_rec = rec.clone();
    let agent = AcpAgent::from_str(AGENT).expect("spawn conformance agent");

    let result = Client
        .builder()
        .on_receive_notification(
            async move |note: SessionNotification, _cx| {
                match note.update {
                    SessionUpdate::AgentMessageChunk(chunk) => {
                        if let ContentBlock::Text(t) = chunk.content {
                            note_rec.text.lock().await.push_str(&t.text);
                        }
                    }
                    SessionUpdate::ToolCall(tc) => {
                        note_rec.tools.lock().await.push(tc.title);
                    }
                    _ => {}
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _connection| {
                perm_rec.perms.fetch_add(1, Ordering::SeqCst);
                let allow = req
                    .options
                    .iter()
                    .find(|o| o.kind == PermissionOptionKind::AllowOnce)
                    .expect("agent must offer allow_once");
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                        allow.option_id.clone(),
                    )),
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |conn: ConnectionTo<Agent>| async move {
            // initialize
            conn.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            // session/new
            let sid = conn
                .send_request(NewSessionRequest::new(cwd()))
                .block_task()
                .await?
                .session_id;

            // streaming: "ping" -> "pong"
            run_rec.text.lock().await.clear();
            let r = conn
                .send_request(PromptRequest::new(sid.clone(), vec![text_block("ping")]))
                .block_task()
                .await?;
            assert_eq!(r.stop_reason, StopReason::EndTurn);
            assert_eq!(run_rec.text.lock().await.as_str(), "pong");

            // tool telemetry: "tool" -> one ToolCall update + "ran tool"
            run_rec.text.lock().await.clear();
            conn.send_request(PromptRequest::new(sid.clone(), vec![text_block("tool")]))
                .block_task()
                .await?;
            {
                let tools = run_rec.tools.lock().await;
                assert_eq!(tools.len(), 1, "expected exactly one tool telemetry event");
                assert!(tools[0].contains("Bash"), "tool title: {}", tools[0]);
            }
            assert_eq!(run_rec.text.lock().await.as_str(), "ran tool");

            // permission: "permit" -> client receives request, allows, agent continues
            run_rec.text.lock().await.clear();
            conn.send_request(PromptRequest::new(sid.clone(), vec![text_block("permit")]))
                .block_task()
                .await?;
            assert_eq!(run_rec.perms.load(Ordering::SeqCst), 1);
            assert_eq!(run_rec.text.lock().await.as_str(), "allowed");

            // cancellation (reshard's real model): the client abandons an
            // in-flight prompt and drops the connection, which terminates the
            // provider. We assert the prompt genuinely stays in-flight, the
            // cancel notification sends without error, and the path never hangs.
            run_rec.text.lock().await.clear();
            let hang = conn
                .send_request(PromptRequest::new(sid.clone(), vec![text_block("hang")]))
                .block_task();
            let inflight = tokio::time::timeout(Duration::from_millis(400), hang).await;
            assert!(
                inflight.is_err(),
                "hang prompt must stay in-flight, not self-complete"
            );
            conn.send_notification(CancelNotification::new(sid.clone()))?;
            assert!(
                run_rec.text.lock().await.is_empty(),
                "abandoned turn must not stream text"
            );

            // Returning drops the connection, which kills the provider process
            // group (agent-client-protocol ChildGuard).
            Ok(())
        })
        .await;

    result.expect("conformance contract must pass");
}

/// A provider that crashes mid-turn surfaces as an error (not a hang), and a
/// fresh connection to a new process recovers.
#[tokio::test]
async fn conformance_crash_then_recovers() {
    // Connection A: crash mid-turn.
    let agent = AcpAgent::from_str(AGENT).expect("spawn conformance agent");
    let crashed = tokio::time::timeout(
        Duration::from_secs(10),
        Client
            .builder()
            .on_receive_notification(
                async move |_n: SessionNotification, _cx| Ok(()),
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |_r: RequestPermissionRequest, responder, _c| {
                    responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(agent, move |conn: ConnectionTo<Agent>| async move {
                conn.send_request(InitializeRequest::new(ProtocolVersion::V1))
                    .block_task()
                    .await?;
                let sid = conn
                    .send_request(NewSessionRequest::new(cwd()))
                    .block_task()
                    .await?
                    .session_id;
                conn.send_request(PromptRequest::new(sid, vec![text_block("crash")]))
                    .block_task()
                    .await?;
                Ok(())
            }),
    )
    .await
    .expect("crashed provider must not hang the client");
    assert!(
        crashed.is_err(),
        "crashed provider must surface as an error"
    );

    // Connection B: a fresh process still works.
    let text = Arc::new(Mutex::new(String::new()));
    let note_text = text.clone();
    let run_text = text.clone();
    let agent = AcpAgent::from_str(AGENT).expect("spawn conformance agent");
    let recovered = Client
        .builder()
        .on_receive_notification(
            async move |note: SessionNotification, _cx| {
                if let SessionUpdate::AgentMessageChunk(chunk) = note.update {
                    if let ContentBlock::Text(t) = chunk.content {
                        note_text.lock().await.push_str(&t.text);
                    }
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |_r: RequestPermissionRequest, responder, _c| {
                responder.respond(RequestPermissionResponse::new(
                    RequestPermissionOutcome::Cancelled,
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |conn: ConnectionTo<Agent>| async move {
            conn.send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let sid = conn
                .send_request(NewSessionRequest::new(cwd()))
                .block_task()
                .await?
                .session_id;
            conn.send_request(PromptRequest::new(sid, vec![text_block("ping")]))
                .block_task()
                .await?;
            assert_eq!(run_text.lock().await.as_str(), "pong");
            Ok(())
        })
        .await;
    recovered.expect("fresh connection after a crash must recover");
}
