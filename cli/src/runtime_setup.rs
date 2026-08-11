use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use owo_colors::OwoColorize;
use rebeam_runtime::{discover_local, Availability, RuntimeReport};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
struct SupervisorConfig {
    version: u8,
    #[serde(default)]
    agents: Vec<ConfiguredAgent>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
struct ConfiguredAgent {
    name: String,
    runtime: String,
    project: PathBuf,
    permission_policy: String,
}

#[derive(Default, Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    agents: Vec<LegacyAgent>,
}

#[derive(Deserialize)]
struct LegacyAgent {
    name: String,
    #[serde(default)]
    provider: Option<String>,
    exec: String,
    #[serde(default)]
    project: Option<PathBuf>,
}

pub fn migrate_legacy_config() -> Result<usize> {
    let legacy = rebeam_home().join("rebeam.toml");
    if !legacy.is_file() {
        return Ok(0);
    }
    let raw = std::fs::read_to_string(&legacy)
        .with_context(|| format!("reading legacy config {}", legacy.display()))?;
    let old: LegacyConfig = toml::from_str(&raw)
        .with_context(|| format!("parsing legacy config {}", legacy.display()))?;
    let path = supervisor_config_path();
    let mut current = load_supervisor_config_at(&path)?;
    let mut added = 0;
    for agent in old.agents {
        if current
            .agents
            .iter()
            .any(|existing| existing.name == agent.name)
        {
            continue;
        }
        validate_agent_name(&agent.name)?;
        let runtime = agent
            .provider
            .unwrap_or_else(|| crate::provider_for_exec(&agent.exec));
        let project = agent
            .project
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        current.agents.push(ConfiguredAgent {
            name: agent.name,
            runtime,
            project,
            permission_policy: "ask".into(),
        });
        added += 1;
    }
    if added > 0 {
        current.version = 1;
        current.agents.sort_by(|a, b| a.name.cmp(&b.name));
        atomic_write(&path, toml::to_string_pretty(&current)?.as_bytes())?;
    }
    Ok(added)
}

pub fn has_configuration() -> bool {
    supervisor_config_path().is_file()
}

/// Install the Claude subscription ACP sidecar into Rebeam's private runtime
/// directory. This is intentionally a first-pass bootstrap using uv; the
/// directory is a stable contract that can later hold a bundled/signed
/// artifact without changing the CLI or ACP supervisor.
pub async fn install(id: &str) -> Result<()> {
    if id != "claude-subscription" && id != "claude" {
        bail!("no managed installer exists for runtime {id:?}");
    }
    let uv = which("uv").context(
        "installing Claude subscription support requires `uv`; install uv or use a bundled Rebeam runtime",
    )?;
    let runtime_dir = rebeam_home().join("runtimes/claude-subscription");
    let venv = runtime_dir.join("venv");
    std::fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("creating {}", runtime_dir.display()))?;
    let status = tokio::process::Command::new(&uv)
        .args(["venv", "--clear", "--python", "3.12"])
        .arg(&venv)
        .status()
        .await
        .context("creating the Claude adapter Python environment")?;
    if !status.success() {
        bail!("uv failed to create the Claude adapter environment ({status})");
    }
    let python = if cfg!(windows) {
        venv.join("Scripts/python.exe")
    } else {
        venv.join("bin/python")
    };
    let status = tokio::process::Command::new(&uv)
        .args(["pip", "install", "--python"])
        .arg(&python)
        .args([
            "claude-code-acp==0.5.1",
            "agent-client-protocol==0.7.1",
            "claude-agent-sdk==0.1.29",
        ])
        .status()
        .await
        .context("installing claude-code-acp into the managed environment")?;
    if !status.success() {
        bail!("uv failed to install claude-code-acp ({status})");
    }
    let adapter = if cfg!(windows) {
        venv.join("Scripts/claude-code-acp.exe")
    } else {
        venv.join("bin/claude-code-acp")
    };
    if !adapter.is_file() {
        bail!("uv completed but did not create {}", adapter.display());
    }
    println!("{} installed Claude subscription adapter", "✓".green());
    println!("  {}", adapter.display());
    Ok(())
}

fn which(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|path| path.is_file())
}

pub async fn runtimes(
    client: &reqwest::Client,
    relay: &str,
    json: bool,
    refresh: bool,
) -> Result<()> {
    let reports = discover_local(refresh).await.map_err(anyhow::Error::msg)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        print_reports(&reports);
    }
    let selected = configured_runtime_ids()?;
    upload_inventory(client, relay, &reports, &selected).await?;
    Ok(())
}

pub async fn doctor(id: &str, json: bool) -> Result<()> {
    let reports = discover_local(true).await.map_err(anyhow::Error::msg)?;
    let report = find_report(&reports, id)
        .with_context(|| format!("unknown runtime {id:?}; run `rebeam runtimes`"))?;
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }
    print_report(report);
    if report.diagnostics.is_empty() {
        println!("\n{} no readiness problems found", "✓".green());
    } else {
        println!();
        for diagnostic in &report.diagnostics {
            println!("{} {}", "•".yellow(), diagnostic.message);
            if let Some(remediation) = &diagnostic.remediation {
                println!("  {}", remediation.dimmed());
            }
        }
    }
    Ok(())
}

pub async fn enable(
    client: &reqwest::Client,
    relay: &str,
    id: &str,
    project: &Path,
    name: &str,
) -> Result<()> {
    let reports = discover_local(true).await.map_err(anyhow::Error::msg)?;
    let report = find_report(&reports, id)
        .with_context(|| format!("unknown runtime {id:?}; run `rebeam runtimes`"))?;
    ensure_ready(report)?;
    let project = validate_project(project)?;
    validate_agent_name(name)?;
    save_agent(ConfiguredAgent {
        name: name.into(),
        runtime: report.id.to_string(),
        project,
        permission_policy: "ask".into(),
    })?;
    let selected = configured_runtime_ids()?;
    upload_inventory(client, relay, &reports, &selected).await?;
    println!(
        "{} configured {} as {}",
        "✓".green(),
        report.label.bold(),
        name.bold()
    );
    println!("  permission policy: {}", "Ask in Rebeam".green());
    println!("  The long-lived supervisor will activate this profile in Phase 5.");
    Ok(())
}

pub async fn setup(
    client: &reqwest::Client,
    relay: &str,
    requested: Vec<String>,
    project: Option<PathBuf>,
    agent_name: Option<String>,
) -> Result<()> {
    println!("{}\n", "Rebeam setup".bold());
    let reports = discover_local(true).await.map_err(anyhow::Error::msg)?;
    print_reports(&reports);

    let selected = if requested.is_empty() {
        prompt_runtime_selection(&reports)?
    } else {
        requested
    };
    if selected.is_empty() {
        bail!("no runtimes selected");
    }
    if agent_name.is_some() && selected.len() != 1 {
        bail!("--agent-name can be used only with one --runtime");
    }

    let shared_project = match project {
        Some(path) => Some(validate_project(&path)?),
        None => None,
    };
    for id in &selected {
        let report = find_report(&reports, id)
            .with_context(|| format!("unknown runtime {id:?}; run `rebeam runtimes`"))?;
        ensure_ready(report)?;
        let project = match &shared_project {
            Some(path) => path.clone(),
            None => prompt_project(&report.label)?,
        };
        let name = match &agent_name {
            Some(name) => name.clone(),
            None => prompt(
                &format!("Agent name for {}", report.label),
                &format!("{}-main", report.id),
            )?,
        };
        validate_agent_name(&name)?;
        save_agent(ConfiguredAgent {
            name,
            runtime: report.id.to_string(),
            project,
            permission_policy: "ask".into(),
        })?;
    }

    let configured = configured_runtime_ids()?;
    upload_inventory(client, relay, &reports, &configured).await?;
    println!("\n{} runtime configuration saved", "✓".green());
    println!("  Permission policy: {}", "Ask in Rebeam".green());
    println!(
        "  Automatic service startup remains disabled until the approval broker can enforce that policy."
    );
    Ok(())
}

fn print_reports(reports: &[RuntimeReport]) {
    println!("{}\n", "Detected agent runtimes".bold());
    for report in reports {
        print_report(report);
    }
}

fn print_report(report: &RuntimeReport) {
    let marker = match report.availability {
        Availability::Ready => "●".green().to_string(),
        Availability::NotInstalled => "○".dimmed().to_string(),
        _ => "●".yellow().to_string(),
    };
    println!(
        "{} {:<14} {}",
        marker,
        report.label.bold(),
        availability_label(report.availability)
    );
    if let Some(path) = &report.binary_path {
        println!("  {}", path.display().to_string().dimmed());
    }
    if let Some(version) = &report.version {
        println!("  {}", version.dimmed());
    }
    if report.capabilities.subscription_compatible {
        println!("  {}", "subscription compatible".dimmed());
    }
    if report.binary_path.is_some()
        && !matches!(
            report.adapter,
            rebeam_runtime::AdapterStatus::Missing | rebeam_runtime::AdapterStatus::Unsupported
        )
        && report.capabilities.enforceable_tool_approvals
    {
        println!("  {}", "approval bridge supported".dimmed());
    }
    println!();
}

fn availability_label(availability: Availability) -> &'static str {
    match availability {
        Availability::Ready => "Ready",
        Availability::LoginRequired => "Login required",
        Availability::AdapterMissing => "Adapter missing",
        Availability::UnsupportedVersion => "Unsupported version/capability",
        Availability::ConfigInvalid => "Configuration invalid",
        Availability::NotInstalled => "Not installed",
        Availability::ProbeFailed => "Probe failed",
    }
}

fn find_report<'a>(reports: &'a [RuntimeReport], id: &str) -> Option<&'a RuntimeReport> {
    reports.iter().find(|report| {
        report.id.as_str().eq_ignore_ascii_case(id)
            || report
                .id
                .as_str()
                .strip_prefix("custom:")
                .is_some_and(|custom| custom.eq_ignore_ascii_case(id))
    })
}

fn ensure_ready(report: &RuntimeReport) -> Result<()> {
    if report.availability == Availability::Ready {
        return Ok(());
    }
    let hint = report
        .diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.as_str())
        .unwrap_or("runtime is not ready");
    bail!(
        "{} is not ready ({}): {hint}. Run `rebeam runtime doctor {}`.",
        report.label,
        availability_label(report.availability),
        report.id
    )
}

fn prompt_runtime_selection(reports: &[RuntimeReport]) -> Result<Vec<String>> {
    if !std::io::stdin().is_terminal() {
        bail!("interactive setup needs a terminal; pass one or more --runtime values");
    }
    let defaults = reports
        .iter()
        .filter(|report| report.availability == Availability::Ready)
        .map(|report| report.id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let value = prompt("Select runtimes (comma-separated)", &defaults)?;
    Ok(value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect())
}

fn prompt_project(label: &str) -> Result<PathBuf> {
    let default = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let value = prompt(
        &format!("Project folder for {label}"),
        &default.display().to_string(),
    )?;
    validate_project(Path::new(&value))
}

fn prompt(label: &str, default: &str) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        bail!("{label} is required in non-interactive setup");
    }
    print!("{label} [{default}]: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let value = line.trim();
    Ok(if value.is_empty() { default } else { value }.to_string())
}

fn validate_project(path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("project folder {} does not exist", path.display()))?;
    if !canonical.is_dir() {
        bail!("project path {} is not a directory", canonical.display());
    }
    Ok(canonical)
}

fn validate_agent_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid {
        bail!("agent name must contain only letters, digits, hyphens, and underscores");
    }
    Ok(())
}

fn configured_runtime_ids() -> Result<Vec<String>> {
    Ok(load_supervisor_config()?
        .agents
        .into_iter()
        .map(|agent| agent.runtime)
        .collect())
}

fn save_agent(agent: ConfiguredAgent) -> Result<()> {
    let path = supervisor_config_path();
    save_agent_at(&path, agent)
}

fn save_agent_at(path: &Path, agent: ConfiguredAgent) -> Result<()> {
    let mut config = load_supervisor_config_at(path)?;
    config.version = 1;
    config.agents.retain(|existing| existing.name != agent.name);
    config.agents.push(agent);
    config
        .agents
        .sort_by(|left, right| left.name.cmp(&right.name));
    let encoded = toml::to_string_pretty(&config)?;
    atomic_write(path, encoded.as_bytes())
}

fn load_supervisor_config() -> Result<SupervisorConfig> {
    let path = supervisor_config_path();
    load_supervisor_config_at(&path)
}

fn load_supervisor_config_at(path: &Path) -> Result<SupervisorConfig> {
    if !path.exists() {
        return Ok(SupervisorConfig::default());
    }
    let encoded =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&encoded).with_context(|| format!("parsing {}", path.display()))
}

async fn upload_inventory(
    client: &reqwest::Client,
    relay: &str,
    reports: &[RuntimeReport],
    selected: &[String],
) -> Result<()> {
    let body = serde_json::json!({
        "runtimes": reports.iter().map(|report| {
            report.inventory_item(selected.iter().any(|id| id == report.id.as_str()))
        }).collect::<Vec<_>>()
    });
    let response = client
        .post(format!("{relay}/machines/runtimes"))
        .json(&body)
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) if response.status() == reqwest::StatusCode::UNAUTHORIZED => {
            // Discovery remains useful before authentication; setup itself
            // authenticates first, so this path is primarily `runtimes`.
            Ok(())
        }
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("relay rejected runtime inventory ({status}): {body}")
        }
        Err(error) => {
            eprintln!(
                "{} could not report runtime inventory to the relay: {error}",
                "warning".yellow()
            );
            Ok(())
        }
    }
}

fn rebeam_home() -> PathBuf {
    rebeam_runtime::rebeam_home()
}

fn supervisor_config_path() -> PathBuf {
    rebeam_home().join("supervisor.toml")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("configuration path has no parent")?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let temporary = parent.join(format!(".supervisor-{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths_are_validated_before_configuration() {
        assert!(validate_agent_name("claude-main").is_ok());
        assert!(validate_agent_name("../../bad").is_err());
        assert!(validate_project(Path::new(".")).is_ok());
    }

    #[test]
    fn supervisor_profiles_are_atomic_and_replace_by_name() {
        let root = std::env::temp_dir().join(format!(
            "rebeam-setup-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let path = root.join("supervisor.toml");
        let agent = |runtime: &str| ConfiguredAgent {
            name: "main".into(),
            runtime: runtime.into(),
            project: std::env::current_dir().unwrap(),
            permission_policy: "ask".into(),
        };
        save_agent_at(&path, agent("claude")).unwrap();
        save_agent_at(&path, agent("opencode")).unwrap();
        let saved = load_supervisor_config_at(&path).unwrap();
        assert_eq!(saved.version, 1);
        assert_eq!(saved.agents.len(), 1);
        assert_eq!(saved.agents[0].runtime, "opencode");
        assert_eq!(saved.agents[0].permission_policy, "ask");
        let encoded = std::fs::read_to_string(&path).unwrap();
        assert!(!encoded.contains("dangerously-skip-permissions"));
        let _ = std::fs::remove_dir_all(root);
    }
}
