use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::sync::RwLock;

use crate::catalog::{self, AdapterRequirementOwned, AuthProbeOwned, RuntimeDefinition};
use crate::probe::{acp_initialize_probe, command_probe, ProbeOutput};
use crate::types::{
    AdapterStatus, AuthStatus, Availability, CustomRuntimeDefinition, Diagnostic, DiagnosticLevel,
    LaunchCommand, RuntimeReport,
};

const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const CACHE_TTL: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub struct DiscoveryOptions {
    pub refresh: bool,
    pub custom: Vec<CustomRuntimeDefinition>,
    pub probe_timeout: Duration,
    /// Trusted override for tests and headless deployments. Tauri never accepts
    /// this value from the frontend.
    pub search_path: Option<Vec<PathBuf>>,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            refresh: false,
            custom: Vec::new(),
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            search_path: None,
        }
    }
}

#[derive(Clone)]
struct CachedReports {
    key: String,
    at: Instant,
    reports: Vec<RuntimeReport>,
}

static CACHE: OnceLock<RwLock<Option<CachedReports>>> = OnceLock::new();

pub async fn discover(options: DiscoveryOptions) -> Vec<RuntimeReport> {
    let search_paths = match options.search_path.clone() {
        Some(paths) => paths,
        None => recovered_search_path(options.probe_timeout).await,
    };
    let key = cache_key(&search_paths);
    let cacheable = options.custom.is_empty()
        && options.search_path.is_none()
        && options.probe_timeout == DEFAULT_PROBE_TIMEOUT;
    if cacheable && !options.refresh {
        let cache = CACHE.get_or_init(|| RwLock::new(None)).read().await;
        if let Some(cached) = cache.as_ref() {
            if cached.key == key && cached.at.elapsed() < CACHE_TTL {
                return cached.reports.clone();
            }
        }
    }

    let mut definitions = catalog::builtins();
    let mut invalid_custom = Vec::new();
    for custom in options.custom {
        match catalog::custom(custom.clone()) {
            Ok(definition) => definitions.push(definition),
            Err(error) => invalid_custom.push(invalid_custom_report(custom, error)),
        }
    }

    let sibling = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let mut jobs = FuturesUnordered::new();
    for definition in definitions {
        jobs.push(discover_one(
            definition,
            search_paths.clone(),
            sibling.clone(),
            options.probe_timeout,
        ));
    }
    let mut reports = Vec::new();
    while let Some(report) = jobs.next().await {
        reports.push(report);
    }
    reports.extend(invalid_custom);
    reports.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));

    if cacheable {
        *CACHE.get_or_init(|| RwLock::new(None)).write().await = Some(CachedReports {
            key,
            at: Instant::now(),
            reports: reports.clone(),
        });
    }
    reports
}

async fn discover_one(
    definition: RuntimeDefinition,
    search_paths: Vec<PathBuf>,
    sibling: Option<PathBuf>,
    timeout: Duration,
) -> RuntimeReport {
    let mut diagnostics = Vec::new();
    let binary = resolve_any(&definition.commands, &search_paths, None);
    let binary = match binary {
        Some(binary) => binary,
        None => {
            if let Some(legacy) = resolve_any(&definition.legacy_commands, &search_paths, None) {
                return RuntimeReport {
                    id: definition.id,
                    label: definition.label,
                    binary_path: Some(legacy),
                    version: None,
                    availability: Availability::ConfigInvalid,
                    auth: AuthStatus::Unknown,
                    adapter: AdapterStatus::Unsupported,
                    capabilities: definition.capabilities,
                    launch: None,
                    diagnostics: vec![Diagnostic::error(
                        "legacy_runtime",
                        "Only a legacy non-ACP runtime was found; it cannot satisfy the Reshard runtime contract.",
                    )
                    .remediation("Install the runtime's ACP executable or adapter.")],
                };
            }
            return RuntimeReport {
                id: definition.id,
                label: definition.label,
                binary_path: None,
                version: None,
                availability: Availability::NotInstalled,
                auth: AuthStatus::Unknown,
                adapter: adapter_when_absent(&definition.adapter),
                capabilities: definition.capabilities,
                launch: None,
                diagnostics: vec![Diagnostic::warning(
                    "not_installed",
                    "No matching executable was found in the terminal or login-shell PATH.",
                )],
            };
        }
    };

    let (adapter, launch_command) = match &definition.adapter {
        AdapterRequirementOwned::None => (
            AdapterStatus::NotRequired,
            Some(LaunchCommand {
                command: binary.clone(),
                args: definition.launch_args.clone(),
            }),
        ),
        AdapterRequirementOwned::Executable {
            commands,
            install_hint,
        } => {
            // Prefer an explicitly discovered adapter (including a project or
            // test fixture adapter). The managed subscription sidecar is the
            // fallback when no adapter is available on PATH.
            let adapter = resolve_any(commands, &search_paths, sibling.as_deref())
                .or_else(|| managed_subscription_adapter(definition.id.as_str()));
            match adapter {
                Some(adapter) => (
                    AdapterStatus::Ready,
                    Some(LaunchCommand {
                        command: adapter,
                        args: definition.launch_args.clone(),
                    }),
                ),
                None => {
                    diagnostics.push(
                        Diagnostic::error(
                            "adapter_missing",
                            "The provider CLI is installed, but its ACP adapter is missing.",
                        )
                        .remediation(install_hint),
                    );
                    (AdapterStatus::Missing, None)
                }
            }
        }
    };

    let version_args = definition.version_args.clone();
    let auth_probe = definition.auth_probe.clone();
    let capability_flag = definition.capability_help_flag.clone();
    let version_future = command_probe(&binary, &version_args, timeout);
    let auth_future = run_auth_probe(&binary, &auth_probe, timeout);
    let capability_future = async {
        match capability_flag {
            Some(flag) => command_probe(&binary, &["--help".into()], timeout)
                .await
                .map(|output| (flag, output)),
            None => Ok((
                String::new(),
                ProbeOutput {
                    success: true,
                    text: String::new(),
                },
            )),
        }
    };
    let (version_result, auth_result, capability_result) =
        tokio::join!(version_future, auth_future, capability_future);

    let version = match version_result {
        Ok(output) if output.success => first_line(&output.text),
        Ok(output) => {
            diagnostics.push(Diagnostic::error(
                "version_probe_failed",
                bounded_message("The version probe failed", &output.text),
            ));
            None
        }
        Err(error) => {
            diagnostics.push(Diagnostic::error("version_probe_failed", error));
            None
        }
    };

    let auth = match auth_result {
        Ok(status) => status,
        Err(error) => {
            diagnostics.push(Diagnostic::error("auth_probe_failed", error).remediation(
                "Run the provider's login command, then run `reshard runtime doctor` again.",
            ));
            AuthStatus::ProbeFailed
        }
    };

    let mut capabilities = definition.capabilities;
    let managed_subscription = launch_command.as_ref().is_some_and(|launch| {
        launch
            .command
            .to_string_lossy()
            .contains("runtimes/claude-subscription/venv")
    });
    if let Some(flag) = definition.capability_help_flag.as_deref() {
        if managed_subscription {
            // The managed Python adapter enforces permissions through the
            // Claude SDK callback; it does not depend on the CLI's optional
            // --permission-prompt-tool flag.
        } else {
            match capability_result {
                Ok((_, output)) if output.success && output.text.contains(flag) => {}
                Ok((_, output)) => {
                    capabilities.enforceable_tool_approvals = false;
                    diagnostics.push(
                    Diagnostic::error(
                        "approval_capability_missing",
                        bounded_message(
                            "This version does not expose the required permission-prompt capability",
                            &output.text,
                        ),
                    )
                    .remediation("Update the provider CLI before enabling it with Reshard's Ask policy."),
                );
                }
                Err(error) => {
                    capabilities.enforceable_tool_approvals = false;
                    diagnostics.push(Diagnostic::error("capability_probe_failed", error));
                }
            }
        }
    }
    // Conformance gate: a provider may only keep its enforceable-tool-approvals
    // claim if it is gated by a CLI capability probe (handled above via
    // `capability_help_flag`) or has been validated against the ACP conformance
    // contract (`cli/tests/conformance.rs`). A native-ACP provider that claims
    // enforceable approvals without either is downgraded to capability-limited
    // under the default Ask policy — Reshard must not trust an unverified adapter
    // to honor `session/request_permission`.
    if capabilities.enforceable_tool_approvals
        && definition.capability_help_flag.is_none()
        && !definition.conformance_verified
    {
        capabilities.enforceable_tool_approvals = false;
        diagnostics.push(
            Diagnostic::error(
                "approval_capability_unverified",
                "This runtime claims enforceable tool approvals but has not passed the ACP conformance contract.",
            )
            .remediation(
                "Validate the adapter with the Reshard conformance suite (cli/tests/conformance.rs) before enabling it under the Ask policy.",
            ),
        );
    } else if !capabilities.enforceable_tool_approvals && definition.capability_help_flag.is_none() {
        diagnostics.push(
            Diagnostic::error(
                "approval_capability_not_declared",
                "This runtime has not declared an enforceable tool-approval boundary.",
            )
            .remediation(
                "Declare enforceableToolApprovals only after the custom ACP adapter passes the permission conformance test.",
            ),
        );
    }

    let unsupported = definition
        .minimum_major
        .zip(version.as_deref().and_then(first_number))
        .is_some_and(|(minimum, actual)| actual < minimum);
    if unsupported {
        diagnostics.push(Diagnostic::error(
            "unsupported_version",
            format!(
                "Version {} or newer is required.",
                definition.minimum_major.unwrap_or_default()
            ),
        ));
    }

    let availability = if adapter == AdapterStatus::Missing {
        Availability::AdapterMissing
    } else if unsupported
        || (!capabilities.enforceable_tool_approvals && definition.capability_help_flag.is_some())
    {
        Availability::UnsupportedVersion
    } else if !capabilities.enforceable_tool_approvals {
        Availability::ConfigInvalid
    } else {
        match auth {
            AuthStatus::LoggedIn | AuthStatus::NotApplicable => {
                if version.is_some() {
                    Availability::Ready
                } else {
                    Availability::ProbeFailed
                }
            }
            AuthStatus::LoginRequired => Availability::LoginRequired,
            AuthStatus::Unknown | AuthStatus::ProbeFailed => Availability::ProbeFailed,
        }
    };

    RuntimeReport {
        id: definition.id,
        label: definition.label,
        binary_path: Some(binary),
        version,
        availability,
        auth,
        adapter,
        capabilities,
        launch: launch_command,
        diagnostics,
    }
}

fn managed_subscription_adapter(runtime_id: &str) -> Option<PathBuf> {
    if runtime_id != "claude" {
        return None;
    }
    let home = std::env::var_os("RESHARD_HOME").or_else(|| std::env::var_os("HOME"))?;
    let root = PathBuf::from(home);
    let root = if std::env::var_os("RESHARD_HOME").is_some() {
        root
    } else {
        root.join(".reshard")
    };
    let path = root.join("runtimes/claude-subscription/venv/bin/claude-code-acp");
    path.is_file().then_some(path)
}

async fn run_auth_probe(
    binary: &Path,
    probe: &AuthProbeOwned,
    timeout: Duration,
) -> Result<AuthStatus, String> {
    match probe {
        AuthProbeOwned::Command(args) => {
            let output = command_probe(binary, args, timeout).await?;
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&output.text) {
                if let Some(logged_in) = value.get("loggedIn").and_then(|value| value.as_bool()) {
                    return Ok(if logged_in {
                        AuthStatus::LoggedIn
                    } else {
                        AuthStatus::LoginRequired
                    });
                }
            }
            if output.success {
                return Ok(AuthStatus::LoggedIn);
            }
            let lower = output.text.to_ascii_lowercase();
            if [
                "login",
                "logged out",
                "not logged in",
                "not authenticated",
                "unauthorized",
                "sign in",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
            {
                Ok(AuthStatus::LoginRequired)
            } else {
                Err(bounded_message("Authentication probe failed", &output.text))
            }
        }
        AuthProbeOwned::AcpInitialize(args) => {
            match acp_initialize_probe(binary, args, timeout).await {
                Ok(()) => Ok(AuthStatus::LoggedIn),
                Err(error) => {
                    let lower = error.to_ascii_lowercase();
                    if ["login", "auth", "credential", "unauthorized"]
                        .iter()
                        .any(|needle| lower.contains(needle))
                    {
                        Ok(AuthStatus::LoginRequired)
                    } else {
                        Err(error)
                    }
                }
            }
        }
    }
}

fn adapter_when_absent(requirement: &AdapterRequirementOwned) -> AdapterStatus {
    match requirement {
        AdapterRequirementOwned::None => AdapterStatus::NotRequired,
        AdapterRequirementOwned::Executable { .. } => AdapterStatus::Missing,
    }
}

fn resolve_any(
    commands: &[String],
    search_paths: &[PathBuf],
    preferred: Option<&Path>,
) -> Option<PathBuf> {
    for command in commands {
        let path = catalog::command_path(command);
        if path.is_absolute() || path.components().count() > 1 {
            if executable(&path) {
                return canonical(path);
            }
            continue;
        }
        if let Some(directory) = preferred {
            let candidate = executable_candidate(directory, &path);
            if let Some(candidate) = candidate {
                return canonical(candidate);
            }
        }
        for directory in search_paths {
            if let Some(candidate) = executable_candidate(directory, &path) {
                return canonical(candidate);
            }
        }
    }
    None
}

fn executable_candidate(directory: &Path, command: &Path) -> Option<PathBuf> {
    let candidate = directory.join(command);
    if executable(&candidate) {
        return Some(candidate);
    }
    #[cfg(windows)]
    for extension in ["exe", "cmd", "bat"] {
        let candidate = candidate.with_extension(extension);
        if executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    true
}

fn canonical(path: PathBuf) -> Option<PathBuf> {
    path.canonicalize().ok().or(Some(path))
}

async fn recovered_search_path(timeout: Duration) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    #[cfg(unix)]
    if let Some(login_path) = login_shell_path(timeout).await {
        paths.extend(std::env::split_paths(&login_path));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            paths.push(directory.to_path_buf());
        }
    }
    deduplicate_paths(paths)
}

#[cfg(unix)]
async fn login_shell_path(timeout: Duration) -> Option<std::ffi::OsString> {
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let output = command_probe(
        &shell,
        &[
            "-lic".into(),
            "printf '\\n__RESHARD_PATH__%s' \"$PATH\"".into(),
        ],
        timeout,
    )
    .await
    .ok()?;
    parse_login_shell_path(&output.text)
}

#[cfg(unix)]
fn parse_login_shell_path(output: &str) -> Option<std::ffi::OsString> {
    let path = output.rsplit_once("__RESHARD_PATH__")?.1.trim();
    (!path.is_empty()).then(|| std::ffi::OsString::from(path))
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| !path.as_os_str().is_empty())
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn cache_key(paths: &[PathBuf]) -> String {
    let shell = std::env::var("SHELL").unwrap_or_default();
    format!(
        "{shell}\0{}",
        std::env::join_paths(paths)
            .unwrap_or_default()
            .to_string_lossy()
    )
}

fn first_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(200).collect())
}

fn first_number(value: &str) -> Option<u64> {
    let digits: String = value
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn bounded_message(prefix: &str, details: &str) -> String {
    let details: String = details.chars().take(500).collect();
    if details.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}: {details}")
    }
}

fn invalid_custom_report(custom: CustomRuntimeDefinition, error: String) -> RuntimeReport {
    let safe_id: String = custom
        .id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(40)
        .collect();
    RuntimeReport {
        id: crate::types::RuntimeId::new(format!(
            "custom:invalid-{}",
            if safe_id.is_empty() {
                "definition"
            } else {
                &safe_id
            }
        )),
        label: custom.label,
        binary_path: None,
        version: None,
        availability: Availability::ConfigInvalid,
        auth: AuthStatus::Unknown,
        adapter: AdapterStatus::Unsupported,
        capabilities: custom
            .capabilities
            .unwrap_or_else(crate::types::RuntimeCapabilities::conservative),
        launch: None,
        diagnostics: vec![Diagnostic {
            level: DiagnosticLevel::Error,
            code: "custom_config_invalid".into(),
            message: error,
            remediation: Some("Fix ~/.reshard/runtimes.toml and refresh discovery.".into()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_extract_the_first_numeric_component() {
        assert_eq!(first_number("claude 2.4.1"), Some(2));
        assert_eq!(first_number("release v17"), Some(17));
        assert_eq!(first_number("unknown"), None);
    }

    #[test]
    fn path_deduplication_preserves_precedence() {
        let paths = deduplicate_paths(vec!["/a".into(), "/b".into(), "/a".into()]);
        assert_eq!(paths, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[cfg(unix)]
    #[test]
    fn login_shell_path_recovers_gui_missing_directories() {
        let directory = fixture_directory();
        executable_fixture(&directory, "claude", "exit 0");
        let shell_output = format!(
            "shell startup noise\n__RESHARD_PATH__{}:/usr/bin",
            directory.display()
        );
        let recovered = parse_login_shell_path(&shell_output).unwrap();
        let paths: Vec<PathBuf> = std::env::split_paths(&recovered).collect();
        assert!(resolve_any(&["claude".into()], &[], None).is_none());
        assert_eq!(
            resolve_any(&["claude".into()], &paths, None),
            directory.join("claude").canonicalize().ok()
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    fn fixture_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "reshard-runtime-fixture-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    fn executable_fixture(directory: &Path, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[cfg(unix)]
    fn fixture_options(directory: &Path) -> DiscoveryOptions {
        DiscoveryOptions {
            refresh: true,
            search_path: Some(vec![directory.to_path_buf()]),
            probe_timeout: Duration::from_secs(2),
            custom: Vec::new(),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fixture_reports_ready_logged_out_missing_invalid_and_absent_states() {
        let ready = fixture_directory();
        executable_fixture(
            &ready,
            "claude",
            r#"case "$1" in
  --version) echo '2.4.1 (Claude Code)' ;;
  --help) echo '--permission-prompt-tool' ;;
  auth) exit 0 ;;
esac"#,
        );
        executable_fixture(&ready, "claude-code-acp", "exit 0");
        let reports = discover(fixture_options(&ready)).await;
        let claude = reports
            .iter()
            .find(|report| report.id.as_str() == "claude")
            .unwrap();
        assert_eq!(claude.availability, Availability::Ready, "{claude:#?}");
        assert_eq!(claude.auth, AuthStatus::LoggedIn);

        // Installed but logged out -> LoginRequired. Use Claude: its
        // --permission-prompt-tool gate keeps enforceable approvals, so
        // availability reflects auth rather than the conformance gate (which
        // would otherwise dominate for a native-ACP provider).
        let logged_out = fixture_directory();
        executable_fixture(
            &logged_out,
            "claude",
            r#"case "$1" in
  --version) echo '2.4.1 (Claude Code)' ;;
  --help) echo '--permission-prompt-tool' ;;
  auth) echo 'not logged in' >&2; exit 1 ;;
esac"#,
        );
        executable_fixture(&logged_out, "claude-code-acp", "exit 0");
        let reports = discover(fixture_options(&logged_out)).await;
        let claude = reports
            .iter()
            .find(|report| report.id.as_str() == "claude")
            .unwrap();
        assert_eq!(claude.availability, Availability::LoginRequired);

        let missing_adapter = fixture_directory();
        executable_fixture(
            &missing_adapter,
            "codex",
            r#"if [ "$1" = "--version" ]; then echo 'codex 1.2.3'; else exit 0; fi"#,
        );
        let reports = discover(fixture_options(&missing_adapter)).await;
        let codex = reports
            .iter()
            .find(|report| report.id.as_str() == "codex")
            .unwrap();
        assert_eq!(codex.availability, Availability::AdapterMissing);

        let legacy = fixture_directory();
        executable_fixture(&legacy, "hermes", "echo 'hermes 0.9'");
        let reports = discover(fixture_options(&legacy)).await;
        let hermes = reports
            .iter()
            .find(|report| report.id.as_str() == "hermes")
            .unwrap();
        assert_eq!(hermes.availability, Availability::ConfigInvalid);
        let gemini = reports
            .iter()
            .find(|report| report.id.as_str() == "gemini")
            .unwrap();
        assert_eq!(gemini.availability, Availability::NotInstalled);

        for directory in [ready, logged_out, missing_adapter, legacy] {
            let _ = std::fs::remove_dir_all(directory);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_without_permission_prompt_support_is_not_ready_under_ask() {
        let directory = fixture_directory();
        executable_fixture(
            &directory,
            "claude",
            r#"case "$1" in
  --version) echo '2.4.1 (Claude Code)' ;;
  --help) echo 'usage: claude --print' ;;
  auth) exit 0 ;;
esac"#,
        );
        executable_fixture(&directory, "claude-code-acp", "exit 0");
        let reports = discover(fixture_options(&directory)).await;
        let claude = reports
            .iter()
            .find(|report| report.id.as_str() == "claude")
            .unwrap();
        assert_eq!(claude.availability, Availability::UnsupportedVersion);
        assert!(!claude.capabilities.enforceable_tool_approvals);
        assert!(claude
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "approval_capability_missing"));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_hung_provider_does_not_hide_a_ready_report() {
        let directory = fixture_directory();
        executable_fixture(
            &directory,
            "claude",
            r#"case "$1" in
  --version) echo '2.4.1' ;;
  --help) echo '--permission-prompt-tool' ;;
  auth) exit 0 ;;
esac"#,
        );
        executable_fixture(&directory, "claude-code-acp", "exit 0");
        executable_fixture(&directory, "codex", r#"while :; do :; done"#);
        executable_fixture(&directory, "codex-acp", "exit 0");
        let started = Instant::now();
        let reports = discover(DiscoveryOptions {
            probe_timeout: Duration::from_secs(1),
            ..fixture_options(&directory)
        })
        .await;
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(
            reports
                .iter()
                .find(|report| report.id.as_str() == "claude")
                .unwrap()
                .availability,
            Availability::Ready,
            "{:#?}",
            reports
                .iter()
                .find(|report| report.id.as_str() == "claude")
                .unwrap()
        );
        // Codex is a native-ACP provider with no conformance verification, so
        // the conformance gate marks it capability-limited (ConfigInvalid)
        // regardless of the hung probe. The point of this test — one slow
        // provider neither blocks discovery nor hides Claude's Ready report —
        // still holds.
        assert_eq!(
            reports
                .iter()
                .find(|report| report.id.as_str() == "codex")
                .unwrap()
                .availability,
            Availability::ConfigInvalid
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    /// A native-ACP provider that declares enforceable approvals but has not
    /// passed the ACP conformance contract is downgraded to capability-limited
    /// under the default Ask policy (Phase 6.3).
    #[cfg(unix)]
    #[tokio::test]
    async fn native_acp_provider_without_conformance_is_capability_limited() {
        let directory = fixture_directory();
        // Installed, logged in, adapter present — the only thing missing is
        // conformance verification.
        executable_fixture(
            &directory,
            "codex",
            r#"if [ "$1" = "--version" ]; then echo 'codex 1.2.3'; fi
exit 0"#,
        );
        executable_fixture(&directory, "codex-acp", "exit 0");
        let reports = discover(fixture_options(&directory)).await;
        let codex = reports
            .iter()
            .find(|report| report.id.as_str() == "codex")
            .unwrap();
        assert!(
            !codex.capabilities.enforceable_tool_approvals,
            "unverified native-ACP provider must not keep enforceable approvals: {codex:#?}"
        );
        assert_eq!(codex.availability, Availability::ConfigInvalid);
        assert!(codex
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "approval_capability_unverified"));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[tokio::test]
    async fn invalid_custom_configuration_is_reported_instead_of_executed() {
        let reports = discover(DiscoveryOptions {
            refresh: true,
            search_path: Some(Vec::new()),
            custom: vec![CustomRuntimeDefinition {
                id: "../../bad".into(),
                label: "Bad".into(),
                command: "/tmp/should-not-run".into(),
                args: Vec::new(),
                version_args: Vec::new(),
                capabilities: None,
            }],
            probe_timeout: Duration::from_millis(20),
        })
        .await;
        let custom = reports
            .iter()
            .find(|report| report.id.as_str().starts_with("custom:"))
            .unwrap();
        assert_eq!(custom.availability, Availability::ConfigInvalid);
        assert!(custom.binary_path.is_none());
    }
}
