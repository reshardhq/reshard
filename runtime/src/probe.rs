use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use agent_client_protocol::schema::v1::InitializeRequest;
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

const MAX_CAPTURE_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub(crate) struct ProbeOutput {
    pub success: bool,
    pub text: String,
}

pub(crate) async fn command_probe(
    executable: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<ProbeOutput, String> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("CI", "1")
        .env("NO_COLOR", "1");
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start probe: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "probe stdout was unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "probe stderr was unavailable".to_string())?;

    let task = async move {
        let stdout = read_bounded(stdout);
        let stderr = read_bounded(stderr);
        let (status, stdout, stderr) = tokio::join!(child.wait(), stdout, stderr);
        let status = status.map_err(|error| format!("probe process failed: {error}"))?;
        let mut bytes = stdout.map_err(|error| format!("reading probe output: {error}"))?;
        let stderr = stderr.map_err(|error| format!("reading probe errors: {error}"))?;
        if !bytes.is_empty() && !stderr.is_empty() {
            bytes.push(b'\n');
        }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(bytes.len());
        bytes.extend_from_slice(&stderr[..stderr.len().min(remaining)]);
        Ok(ProbeOutput {
            success: status.success(),
            text: String::from_utf8_lossy(&bytes).trim().to_string(),
        })
    };

    tokio::time::timeout(timeout, task)
        .await
        .map_err(|_| format!("probe exceeded {} ms", timeout.as_millis()))?
}

async fn read_bounded(mut reader: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut captured = Vec::with_capacity(MAX_CAPTURE_BYTES);
    let mut buffer = [0_u8; 4 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    Ok(captured)
}

pub(crate) async fn acp_initialize_probe(
    executable: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<(), String> {
    let config = AcpAgentConfig::new(executable).args(args.iter().cloned());
    let agent = AcpAgent::new(config);
    let probe = agent_client_protocol::Client.builder().connect_with(
        agent,
        |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            Ok(())
        },
    );
    tokio::time::timeout(timeout, probe)
        .await
        .map_err(|_| format!("ACP initialize exceeded {} ms", timeout.as_millis()))?
        .map_err(|error| bounded_error(&error.to_string()))
}

fn bounded_error(value: &str) -> String {
    value.chars().take(2_000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn command_capture_is_bounded_and_times_out() {
        let output = command_probe(
            Path::new("/bin/sh"),
            &["-c".into(), "yes x | head -c 100000".into()],
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert!(output.success);
        assert!(output.text.len() <= MAX_CAPTURE_BYTES);

        let timeout = command_probe(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 2".into()],
            Duration::from_millis(30),
        )
        .await
        .unwrap_err();
        assert!(timeout.contains("exceeded"));
    }
}
