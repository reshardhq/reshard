//! Shared, data-driven discovery for agent runtimes used by both Rebeam CLI
//! and the native desktop shell.

mod catalog;
mod discovery;
mod probe;
mod types;

pub use discovery::{discover, DiscoveryOptions};
pub use types::{
    AdapterStatus, AuthStatus, Availability, CustomRuntimeDefinition, Diagnostic, DiagnosticLevel,
    ExecutionLocus, LaunchCommand, RuntimeCapabilities, RuntimeId, RuntimeInventoryItem,
    RuntimeReport,
};

/// The one runtime-discovery entry point used by both the CLI and Tauri.
/// Keeping config loading here guarantees identical catalog, PATH recovery,
/// probes, caching, and serialized reports in both clients.
pub async fn discover_local(refresh: bool) -> Result<Vec<RuntimeReport>, String> {
    let path = runtime_catalog_path();
    let custom = if path.exists() {
        #[derive(serde::Deserialize, Default)]
        #[serde(deny_unknown_fields)]
        struct Catalog {
            #[serde(default)]
            runtimes: Vec<CustomRuntimeDefinition>,
        }

        let encoded = std::fs::read_to_string(&path)
            .map_err(|error| format!("reading {}: {error}", path.display()))?;
        toml::from_str::<Catalog>(&encoded)
            .map_err(|error| format!("parsing {}: {error}", path.display()))?
            .runtimes
    } else {
        Vec::new()
    };
    Ok(discover(DiscoveryOptions {
        refresh,
        custom,
        ..DiscoveryOptions::default()
    })
    .await)
}

pub fn rebeam_home() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("REBEAM_HOME") {
        return path.into();
    }
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rebeam")
}

pub fn runtime_catalog_path() -> std::path::PathBuf {
    rebeam_home().join("runtimes.toml")
}
