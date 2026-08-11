//! Machine-local approval broker.
//!
//! Provider input stays here. The relay receives only a bounded display and a
//! SHA-256 digest, then returns a one-shot decision for that exact invocation.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use rebeam_core::{Approval, ApprovalDisplay, ApprovalState, Command, Event};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct ApprovalBroker {
    http: reqwest::Client,
    relay: String,
    agent: String,
    chat: String,
    provider: String,
    project: Option<String>,
    run_id: String,
    released: Arc<Mutex<HashSet<String>>>,
}

#[derive(Clone)]
pub struct PendingApproval {
    broker: ApprovalBroker,
    approval: Approval,
    tool_call_id: String,
    digest: String,
}

impl ApprovalBroker {
    pub fn new(
        http: reqwest::Client,
        relay: String,
        agent: String,
        chat: String,
        provider: String,
        project: Option<String>,
    ) -> Self {
        Self {
            http,
            relay,
            agent,
            chat,
            provider,
            project,
            run_id: format!("run_{}", uuid::Uuid::new_v4().simple()),
            released: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Wait for an owner decision and release this invocation at most once.
    pub async fn authorize(
        &self,
        exact_input: &Value,
        tool_call_id: &str,
        tool: &str,
        target: Option<&str>,
        timeout: Duration,
    ) -> Result<bool> {
        let pending = self
            .open(exact_input, tool_call_id, tool, target, timeout)
            .await?;
        pending.wait(timeout).await
    }

    /// Create the durable relay request and return a cancellable handle. The
    /// MCP transport uses this split so EOF from Claude can cancel immediately.
    pub async fn open(
        &self,
        exact_input: &Value,
        tool_call_id: &str,
        tool: &str,
        target: Option<&str>,
        timeout: Duration,
    ) -> Result<PendingApproval> {
        let digest = input_digest(exact_input);
        let display = ApprovalDisplay {
            summary: bounded(&format!("Allow {tool} to run once?"), 500),
            project: self.project.as_deref().map(|value| bounded(value, 300)),
            target: target.map(|value| bounded(&redact_preview(value), 500)),
            command: target
                .filter(|_| tool.eq_ignore_ascii_case("bash") || tool.eq_ignore_ascii_case("shell"))
                .map(|value| bounded(&redact_preview(value), 1_000)),
        };
        let event = self
            .post(Command::RequestApproval {
                agent: self.agent.clone(),
                chat: self.chat.clone(),
                run: self.run_id.clone(),
                tool_call: tool_call_id.to_owned(),
                provider: self.provider.clone(),
                tool: bounded(tool, 80),
                display,
                input_digest: digest.clone(),
                expires_in_ms: timeout.as_millis().clamp(1_000, 24 * 60 * 60 * 1_000) as i64,
            })
            .await?;
        let Event::ApprovalRequested { approval } = event else {
            bail!("relay returned the wrong event for an approval request");
        };
        validate_invocation(&approval, &self.run_id, tool_call_id, &digest)?;

        Ok(PendingApproval {
            broker: self.clone(),
            approval,
            tool_call_id: tool_call_id.to_owned(),
            digest,
        })
    }

    async fn post(&self, command: Command) -> Result<Event> {
        let response = self
            .http
            .post(format!("{}/commands", self.relay))
            .json(&command)
            .send()
            .await
            .with_context(|| format!("cannot reach the relay at {}", self.relay))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("approval relay returned {status}: {body}");
        }
        Ok(response.json().await?)
    }

    fn release_once(&self, approval_id: &str) -> bool {
        self.released
            .lock()
            .expect("approval release gate poisoned")
            .insert(approval_id.to_owned())
    }

    fn release_allowed_once(
        &self,
        approval: &Approval,
        expected_agent: &str,
        expected_chat: &str,
        tool_call_id: &str,
        digest: &str,
    ) -> Result<bool> {
        validate_identity(
            approval,
            expected_agent,
            expected_chat,
            &self.run_id,
            tool_call_id,
            digest,
        )?;
        Ok(approval.state == ApprovalState::Allowed && self.release_once(&approval.id))
    }
}

impl PendingApproval {
    pub async fn wait(&self, timeout: Duration) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let response = self
                .broker
                .http
                .get(format!(
                    "{}/approvals/{}",
                    self.broker.relay, self.approval.id
                ))
                .send()
                .await
                .context("approval recovery request failed")?;
            if !response.status().is_success() {
                bail!("approval recovery failed: {}", response.status());
            }
            let current: Approval = response.json().await?;
            validate_identity(
                &current,
                &self.approval.agent_id,
                &self.approval.chat_id,
                &self.approval.run_id,
                &self.tool_call_id,
                &self.digest,
            )?;
            match current.state {
                ApprovalState::Pending => {}
                ApprovalState::Allowed => {
                    return self.broker.release_allowed_once(
                        &current,
                        &self.approval.agent_id,
                        &self.approval.chat_id,
                        &self.tool_call_id,
                        &self.digest,
                    );
                }
                ApprovalState::Denied | ApprovalState::Expired | ApprovalState::Cancelled => {
                    return Ok(false);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                let _ = self.cancel("local approval wait timed out").await;
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
    }

    pub async fn cancel(&self, reason: &str) -> Result<()> {
        self.broker
            .post(Command::CancelApproval {
                approval: self.approval.id.clone(),
                reason: reason.to_owned(),
            })
            .await
            .map(|_| ())
    }
}

pub fn request_metadata(value: &Value) -> (String, String, Option<String>) {
    let call = value
        .pointer("/toolCall")
        .or_else(|| value.pointer("/tool_call"))
        .unwrap_or(value);
    let id = ["toolCallId", "tool_call_id", "id"]
        .into_iter()
        .find_map(|key| call.get(key).and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
    let title = call.get("title").and_then(Value::as_str).unwrap_or("Tool");
    let (tool, target) = title
        .split_once(": ")
        .map(|(tool, target)| (tool.to_owned(), Some(target.to_owned())))
        .unwrap_or_else(|| (title.to_owned(), None));
    (id, tool, target)
}

pub fn input_digest(value: &Value) -> String {
    let canonical = canonical_json(value);
    format!("{:x}", Sha256::digest(canonical.as_bytes()))
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            format!(
                "{{{}}}",
                entries
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

fn validate_identity(
    approval: &Approval,
    agent: &str,
    chat: &str,
    run: &str,
    tool_call: &str,
    digest: &str,
) -> Result<()> {
    if approval.agent_id != agent || approval.chat_id != chat {
        bail!("approval response does not match the exact local invocation");
    }
    validate_invocation(approval, run, tool_call, digest)
}

fn validate_invocation(
    approval: &Approval,
    run: &str,
    tool_call: &str,
    digest: &str,
) -> Result<()> {
    if approval.run_id != run
        || approval.tool_call_id != tool_call
        || approval.input_digest != digest
    {
        bail!("approval response does not match the exact local invocation");
    }
    Ok(())
}

fn bounded(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max.saturating_sub(3).min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &value[..end])
}

fn redact_preview(value: &str) -> String {
    let source = value.split_whitespace().collect::<Vec<_>>();
    let mut output = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        let word = source[index];
        let lower = word.to_ascii_lowercase();
        if lower.contains("authorization:") {
            let prefix = word
                .find(':')
                .map(|position| &word[..=position])
                .unwrap_or("authorization:");
            output.push(format!("{prefix}[REDACTED]"));
            // Cover the conventional `Authorization: Bearer <credential>`.
            index = (index + 3).min(source.len());
            continue;
        }
        if ["--token", "--password", "--secret", "--api-key", "--apikey"].contains(&lower.as_str())
        {
            output.push(word.to_owned());
            output.push("[REDACTED]".into());
            index += 2;
            continue;
        }
        if ["token=", "password=", "secret=", "api_key=", "apikey="]
            .iter()
            .any(|marker| lower.contains(marker))
        {
            let prefix = word
                .find('=')
                .map(|position| &word[..=position])
                .unwrap_or("");
            output.push(format!("{prefix}[REDACTED]"));
        } else {
            output.push(word.to_owned());
        }
        index += 1;
    }
    output.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_canonical_and_changes_with_exact_input() {
        let first = serde_json::json!({"path":"a", "args":{"z":2,"a":1}});
        let reordered = serde_json::json!({"args":{"a":1,"z":2}, "path":"a"});
        let changed = serde_json::json!({"args":{"a":1,"z":3}, "path":"a"});
        assert_eq!(input_digest(&first), input_digest(&reordered));
        assert_ne!(input_digest(&first), input_digest(&changed));
    }

    #[test]
    fn previews_are_bounded_and_scrub_obvious_credentials() {
        let preview = redact_preview("curl token=super-secret password=hunter2 /safe");
        assert_eq!(preview, "curl token=[REDACTED] password=[REDACTED] /safe");
        assert_eq!(
            redact_preview("curl -H Authorization: Bearer secret https://safe"),
            "curl -H Authorization:[REDACTED] https://safe"
        );
        assert_eq!(
            redact_preview("deploy --api-key secret-value production"),
            "deploy --api-key [REDACTED] production"
        );
        assert!(bounded(&"é".repeat(600), 500).len() <= 500);
    }

    #[test]
    fn fake_provider_releases_one_matching_allowed_call_once() {
        let broker = ApprovalBroker::new(
            reqwest::Client::new(),
            "http://unused".into(),
            "agent".into(),
            "chat".into(),
            "fake".into(),
            None,
        );
        let digest = input_digest(&serde_json::json!({"path":"fixture.txt"}));
        let mut approval = Approval {
            id: "approval-1".into(),
            owner_id: "owner".into(),
            machine_id: "machine".into(),
            agent_id: "agent".into(),
            chat_id: "chat".into(),
            run_id: broker.run_id.clone(),
            tool_call_id: "call-1".into(),
            provider: "fake".into(),
            tool: "FakeWrite".into(),
            display: ApprovalDisplay {
                summary: "Write fixture?".into(),
                project: None,
                target: Some("fixture.txt".into()),
                command: None,
            },
            input_digest: digest.clone(),
            state: ApprovalState::Allowed,
            expires_at: 10,
            created_at: 0,
            resolved_at: Some(1),
            resolved_by: Some("owner".into()),
            resolution_reason: None,
        };
        assert!(broker
            .release_allowed_once(&approval, "agent", "chat", "call-1", &digest)
            .unwrap());
        assert!(!broker
            .release_allowed_once(&approval, "agent", "chat", "call-1", &digest)
            .unwrap());
        approval.id = "approval-2".into();
        approval.state = ApprovalState::Denied;
        assert!(!broker
            .release_allowed_once(&approval, "agent", "chat", "call-1", &digest)
            .unwrap());
        approval.state = ApprovalState::Allowed;
        assert!(broker
            .release_allowed_once(&approval, "agent", "chat", "different-call", &digest)
            .is_err());
        assert!(broker
            .release_allowed_once(&approval, "agent", "chat", "call-1", &"f".repeat(64))
            .is_err());
    }
}
