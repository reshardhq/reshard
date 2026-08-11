#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

const CREDENTIAL_SERVICE: &str = "app.reshard.desktop";
const CREDENTIAL_ACCOUNT: &str = "human-session";

fn legacy_credential_path() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".reshard")
        .join("device-token")
}

#[tauri::command]
fn load_auth_session() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
        .map_err(|error| error.to_string())?;
    match entry.get_password() {
        Ok(session) => Ok(Some(session)),
        Err(keyring::Error::NoEntry) => {
            // Migrate the phase-0 plaintext credential once. It will usually be
            // an expired anonymous token, but moving it before validation makes
            // the storage boundary correct without special-casing old installs.
            let path = legacy_credential_path();
            let legacy = std::fs::read_to_string(&path).ok();
            if let Some(session) = legacy.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                let encoded = serde_json::json!({ "token": session }).to_string();
                entry
                    .set_password(&encoded)
                    .map_err(|error| error.to_string())?;
                let _ = std::fs::remove_file(path);
                return Ok(Some(encoded));
            }
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
fn save_auth_session(session: String) -> Result<(), String> {
    if session.is_empty() || session.len() > 32 * 1_024 {
        return Err("invalid auth session size".into());
    }
    serde_json::from_str::<serde_json::Value>(&session)
        .map_err(|_| "auth session is not valid JSON".to_string())?;
    keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
        .map_err(|error| error.to_string())?
        .set_password(&session)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_auth_session() -> Result<(), String> {
    let entry = keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
        .map_err(|error| error.to_string())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// The frontend can request refresh, but cannot supply an executable path.
/// Custom definitions are read only from the user's local trusted config.
#[tauri::command]
async fn discover_runtimes(refresh: bool) -> Result<Vec<reshard_runtime::RuntimeReport>, String> {
    reshard_runtime::discover_local(refresh).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            load_auth_session,
            save_auth_session,
            delete_auth_session,
            discover_runtimes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
