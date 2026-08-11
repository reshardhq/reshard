use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeId(pub String);

impl RuntimeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuntimeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Ready,
    LoginRequired,
    AdapterMissing,
    UnsupportedVersion,
    ConfigInvalid,
    NotInstalled,
    ProbeFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthStatus {
    LoggedIn,
    LoginRequired,
    Unknown,
    NotApplicable,
    ProbeFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdapterStatus {
    Ready,
    Missing,
    NotRequired,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionLocus {
    LocalProcess,
    RemoteDaemon,
    ExternalService,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    pub native_acp: bool,
    pub adapter_backed: bool,
    pub subscription_compatible: bool,
    pub resumable_sessions: bool,
    pub enforceable_tool_approvals: bool,
    pub cancellation: bool,
    pub model_switching: bool,
    pub maximum_parallelism: u16,
    pub execution_locus: ExecutionLocus,
}

impl RuntimeCapabilities {
    pub(crate) fn conservative() -> Self {
        Self {
            native_acp: true,
            adapter_backed: false,
            subscription_compatible: false,
            resumable_sessions: false,
            enforceable_tool_approvals: false,
            cancellation: false,
            model_switching: false,
            maximum_parallelism: 1,
            execution_locus: ExecutionLocus::LocalProcess,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticLevel {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub code: String,
    pub message: String,
    pub remediation: Option<String>,
}

impl Diagnostic {
    pub(crate) fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            code: code.into(),
            message: message.into(),
            remediation: None,
        }
    }

    pub(crate) fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            code: code.into(),
            message: message.into(),
            remediation: None,
        }
    }

    pub(crate) fn remediation(mut self, value: impl Into<String>) -> Self {
        self.remediation = Some(value.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCommand {
    pub command: PathBuf,
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReport {
    pub id: RuntimeId,
    pub label: String,
    pub binary_path: Option<PathBuf>,
    pub version: Option<String>,
    pub availability: Availability,
    pub auth: AuthStatus,
    pub adapter: AdapterStatus,
    pub capabilities: RuntimeCapabilities,
    pub launch: Option<LaunchCommand>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomRuntimeDefinition {
    pub id: String,
    pub label: String,
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub version_args: Vec<String>,
    #[serde(default)]
    pub capabilities: Option<RuntimeCapabilities>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInventoryItem {
    pub id: RuntimeId,
    pub label: String,
    pub version: Option<String>,
    pub availability: Availability,
    pub auth: AuthStatus,
    pub adapter: AdapterStatus,
    pub capabilities: RuntimeCapabilities,
    pub selected: bool,
}

impl RuntimeReport {
    pub fn inventory_item(&self, selected: bool) -> RuntimeInventoryItem {
        RuntimeInventoryItem {
            id: self.id.clone(),
            label: self.label.clone(),
            version: self.version.clone(),
            availability: self.availability,
            auth: self.auth,
            adapter: self.adapter,
            capabilities: self.capabilities.clone(),
            selected,
        }
    }
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    #[test]
    fn report_json_is_the_cli_tauri_contract() {
        let report = RuntimeReport {
            id: RuntimeId::new("claude"),
            label: "Claude".into(),
            binary_path: Some(PathBuf::from("/usr/local/bin/claude")),
            version: Some("2.1.226".into()),
            availability: Availability::Ready,
            auth: AuthStatus::LoggedIn,
            adapter: AdapterStatus::Ready,
            capabilities: RuntimeCapabilities {
                native_acp: false,
                adapter_backed: true,
                subscription_compatible: true,
                resumable_sessions: true,
                enforceable_tool_approvals: true,
                cancellation: true,
                model_switching: false,
                maximum_parallelism: 1,
                execution_locus: ExecutionLocus::LocalProcess,
            },
            launch: Some(LaunchCommand {
                command: PathBuf::from("/opt/rebeam/claude-code-acp"),
                args: vec!["--stdio".into()],
            }),
            diagnostics: vec![],
        };
        let value = serde_json::to_value(report).expect("runtime report serializes");
        for key in [
            "binaryPath",
            "availability",
            "auth",
            "adapter",
            "capabilities",
            "launch",
            "diagnostics",
        ] {
            assert!(value.get(key).is_some(), "missing shared field {key}");
        }
        let capabilities = value.get("capabilities").unwrap();
        for key in [
            "nativeAcp",
            "adapterBacked",
            "subscriptionCompatible",
            "resumableSessions",
            "enforceableToolApprovals",
            "maximumParallelism",
            "executionLocus",
        ] {
            assert!(
                capabilities.get(key).is_some(),
                "missing capability field {key}"
            );
        }
        assert_eq!(value["availability"], "ready");
        assert_eq!(value["auth"], "loggedIn");
        assert_eq!(value["adapter"], "ready");
    }
}
