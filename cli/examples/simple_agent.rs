//! Minimal ACP *agent* for smoke-testing the `rebeam acp` client.
//!
//! Copied from the agent-client-protocol SDK examples. It only answers
//! `initialize`, which is enough to prove our client spawns it, completes the
//! stdio handshake, and negotiates a protocol version.
//!
//! Run the client against it:
//!   cargo build -p rebeam-cli --examples
//!   cargo run  -p rebeam-cli -- acp \
//!     --command "target/debug/examples/simple_agent" -m "hi"

use agent_client_protocol::schema::v1::{AgentCapabilities, InitializeRequest, InitializeResponse};
use agent_client_protocol::{Agent, Result, Stdio};

#[tokio::main]
async fn main() -> Result<()> {
    Agent
        .builder()
        .name("simple-agent")
        .on_receive_request(
            async move |initialize: InitializeRequest, responder, _connection| {
                responder.respond(
                    InitializeResponse::new(initialize.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}
