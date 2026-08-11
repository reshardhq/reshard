use std::path::PathBuf;

use crate::types::{CustomRuntimeDefinition, ExecutionLocus, RuntimeCapabilities, RuntimeId};

#[derive(Clone, Debug)]
pub(crate) enum AuthProbe {
    Command(Vec<&'static str>),
    AcpInitialize(Vec<&'static str>),
}

#[derive(Clone, Debug)]
pub(crate) enum AdapterRequirement {
    None,
    Executable {
        commands: Vec<&'static str>,
        install_hint: &'static str,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeDefinition {
    pub id: RuntimeId,
    pub label: String,
    pub commands: Vec<String>,
    pub legacy_commands: Vec<String>,
    pub version_args: Vec<String>,
    pub auth_probe: AuthProbeOwned,
    pub adapter: AdapterRequirementOwned,
    pub launch_args: Vec<String>,
    pub minimum_major: Option<u64>,
    pub capabilities: RuntimeCapabilities,
    pub capability_help_flag: Option<String>,
    /// Whether this adapter has been validated against the ACP conformance
    /// contract (`cli/tests/conformance.rs`). A provider that claims enforceable
    /// tool approvals but is neither gated by a CLI capability probe
    /// (`capability_help_flag`) nor conformance-verified is downgraded to
    /// capability-limited under the default Ask policy.
    pub conformance_verified: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum AuthProbeOwned {
    Command(Vec<String>),
    AcpInitialize(Vec<String>),
}

#[derive(Clone, Debug)]
pub(crate) enum AdapterRequirementOwned {
    None,
    Executable {
        commands: Vec<String>,
        install_hint: String,
    },
}

// Catalog rows are easier to audit when every capability is explicit at the
// callsite rather than hidden behind provider-specific defaults.
#[allow(clippy::too_many_arguments)]
fn capabilities(
    native_acp: bool,
    adapter_backed: bool,
    subscription: bool,
    resumable: bool,
    approvals: bool,
    cancellation: bool,
    model_switching: bool,
    parallelism: u16,
) -> RuntimeCapabilities {
    RuntimeCapabilities {
        native_acp,
        adapter_backed,
        subscription_compatible: subscription,
        resumable_sessions: resumable,
        enforceable_tool_approvals: approvals,
        cancellation,
        model_switching,
        maximum_parallelism: parallelism,
        execution_locus: ExecutionLocus::LocalProcess,
    }
}

#[allow(clippy::too_many_arguments)]
fn definition(
    id: &str,
    label: &str,
    commands: &[&str],
    version_args: &[&str],
    auth_probe: AuthProbe,
    adapter: AdapterRequirement,
    launch_args: &[&str],
    capabilities: RuntimeCapabilities,
) -> RuntimeDefinition {
    RuntimeDefinition {
        id: RuntimeId::new(id),
        label: label.into(),
        commands: commands.iter().map(|value| (*value).into()).collect(),
        legacy_commands: Vec::new(),
        version_args: version_args.iter().map(|value| (*value).into()).collect(),
        auth_probe: match auth_probe {
            AuthProbe::Command(args) => {
                AuthProbeOwned::Command(args.into_iter().map(Into::into).collect())
            }
            AuthProbe::AcpInitialize(args) => {
                AuthProbeOwned::AcpInitialize(args.into_iter().map(Into::into).collect())
            }
        },
        adapter: match adapter {
            AdapterRequirement::None => AdapterRequirementOwned::None,
            AdapterRequirement::Executable {
                commands,
                install_hint,
            } => AdapterRequirementOwned::Executable {
                commands: commands.into_iter().map(Into::into).collect(),
                install_hint: install_hint.into(),
            },
        },
        launch_args: launch_args.iter().map(|value| (*value).into()).collect(),
        minimum_major: None,
        capabilities,
        capability_help_flag: None,
        conformance_verified: false,
    }
}

pub(crate) fn builtins() -> Vec<RuntimeDefinition> {
    let mut claude = definition(
        "claude",
        "Claude Code",
        &["claude"],
        &["--version"],
        AuthProbe::Command(vec!["auth", "status"]),
        AdapterRequirement::Executable {
            commands: vec!["claude-code-acp"],
            install_hint: "Reinstall Rebeam so claude-code-acp is next to the rebeam binary.",
        },
        &[],
        capabilities(false, true, true, true, true, true, true, 1),
    );
    claude.minimum_major = Some(2);
    claude.capability_help_flag = Some("--permission-prompt-tool".into());

    let codex = definition(
        "codex",
        "Codex",
        &["codex"],
        &["--version"],
        AuthProbe::Command(vec!["login", "status"]),
        AdapterRequirement::Executable {
            commands: vec!["codex-acp"],
            install_hint:
                "Install codex-acp explicitly; Rebeam will not run an installer during discovery.",
        },
        &[],
        capabilities(false, true, false, true, true, true, true, 4),
    );

    let gemini = definition(
        "gemini",
        "Gemini CLI",
        &["gemini"],
        &["--version"],
        AuthProbe::AcpInitialize(vec!["--experimental-acp"]),
        AdapterRequirement::None,
        &["--experimental-acp"],
        capabilities(true, false, false, true, true, true, true, 4),
    );

    let kimi = definition(
        "kimi",
        "Kimi Code",
        &["kimi"],
        &["--version"],
        AuthProbe::AcpInitialize(vec!["acp"]),
        AdapterRequirement::None,
        &["acp"],
        capabilities(true, false, false, true, true, true, true, 2),
    );

    let mut hermes = definition(
        "hermes",
        "Hermes",
        &["hermes-acp"],
        &["--version"],
        AuthProbe::AcpInitialize(vec![]),
        AdapterRequirement::None,
        &[],
        capabilities(true, false, false, true, true, true, false, 1),
    );
    hermes.legacy_commands = vec!["hermes".into()];

    let opencode = definition(
        "opencode",
        "OpenCode",
        &["opencode"],
        &["--version"],
        AuthProbe::AcpInitialize(vec!["acp"]),
        AdapterRequirement::None,
        &["acp"],
        capabilities(true, false, false, true, true, true, true, 4),
    );

    vec![claude, codex, gemini, kimi, hermes, opencode]
}

pub(crate) fn custom(definition: CustomRuntimeDefinition) -> Result<RuntimeDefinition, String> {
    let valid_id = !definition.id.is_empty()
        && definition.id.len() <= 40
        && definition.id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    if !valid_id {
        return Err(
            "custom runtime id must contain only lowercase letters, digits, and hyphens".into(),
        );
    }
    if definition.label.trim().is_empty() || definition.label.len() > 80 {
        return Err("custom runtime label must be between 1 and 80 characters".into());
    }
    if definition.command.as_os_str().is_empty() {
        return Err("custom runtime command is empty".into());
    }
    if !definition.command.is_absolute() && definition.command.components().count() > 1 {
        return Err(
            "custom runtime command must be an absolute path or a bare executable name".into(),
        );
    }
    if definition.args.len() > 64
        || definition.version_args.len() > 16
        || definition
            .args
            .iter()
            .chain(&definition.version_args)
            .any(|argument| argument.len() > 4_096 || argument.contains('\0'))
    {
        return Err("custom runtime arguments exceed the safe limits".into());
    }
    let command = definition.command.to_string_lossy().to_string();
    Ok(RuntimeDefinition {
        id: RuntimeId::new(format!("custom:{}", definition.id)),
        label: definition.label,
        commands: vec![command],
        legacy_commands: Vec::new(),
        version_args: if definition.version_args.is_empty() {
            vec!["--version".into()]
        } else {
            definition.version_args
        },
        auth_probe: AuthProbeOwned::AcpInitialize(definition.args.clone()),
        adapter: AdapterRequirementOwned::None,
        launch_args: definition.args,
        minimum_major: None,
        capabilities: definition
            .capabilities
            .unwrap_or_else(RuntimeCapabilities::conservative),
        capability_help_flag: None,
        // Custom user-defined adapters are never pre-verified; they must pass
        // the conformance contract before claiming enforceable approvals.
        conformance_verified: false,
    })
}

pub(crate) fn command_path(command: &str) -> PathBuf {
    PathBuf::from(command)
}
