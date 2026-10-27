//! Applies and restores the desktop system proxy.
//!
//! GNOME (Linux) is the only supported platform for now. The previous
//! settings are saved before anything is changed, so stopping the core (or
//! closing the app) puts the user's original configuration back.

#[cfg(target_os = "linux")]
use std::{fs, path::PathBuf, process::Command};

#[cfg(target_os = "linux")]
use serde_json::{json, Value};

#[cfg(target_os = "linux")]
const BACKUP_FILE: &str = "system-proxy-backup.json";

#[cfg(target_os = "linux")]
const SAVED_KEYS: &[(&str, &str)] = &[
    ("org.gnome.system.proxy", "mode"),
    ("org.gnome.system.proxy", "use-same-proxy"),
    ("org.gnome.system.proxy.http", "host"),
    ("org.gnome.system.proxy.http", "port"),
    ("org.gnome.system.proxy.https", "host"),
    ("org.gnome.system.proxy.https", "port"),
    ("org.gnome.system.proxy.socks", "host"),
    ("org.gnome.system.proxy.socks", "port"),
    ("org.gnome.system.proxy.ftp", "host"),
    ("org.gnome.system.proxy.ftp", "port"),
];

/// Points the GNOME system proxy at the local HTTP and SOCKS ports.
#[cfg(target_os = "linux")]
pub fn enable(http_port: u16, socks_port: u16) -> Result<(), String> {
    let backup = backup_path();
    if !backup.is_file() {
        save_backup(&backup)?;
    }

    let result = set_proxy(http_port, socks_port);
    if result.is_err() {
        // Never leave the system proxy half-applied.
        let _ = restore();
    }
    result
}

#[cfg(not(target_os = "linux"))]
pub fn enable(_http_port: u16, _socks_port: u16) -> Result<(), String> {
    Err("System proxy integration is currently only available for GNOME on Linux.".to_string())
}

/// Restores the settings captured by [`enable`].
///
/// When no backup exists nothing was changed, so this is a no-op.
#[cfg(target_os = "linux")]
pub fn disable() -> Result<(), String> {
    restore()
}

#[cfg(not(target_os = "linux"))]
pub fn disable() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_proxy(http_port: u16, socks_port: u16) -> Result<(), String> {
    let http_port = http_port.to_string();
    let socks_port = socks_port.to_string();

    set("org.gnome.system.proxy", "mode", "manual")?;
    // Without this GNOME would route every protocol through the HTTP proxy
    // and ignore the SOCKS port.
    set("org.gnome.system.proxy", "use-same-proxy", "false")?;
    set("org.gnome.system.proxy.http", "host", "127.0.0.1")?;
    set("org.gnome.system.proxy.http", "port", &http_port)?;
    set("org.gnome.system.proxy.https", "host", "127.0.0.1")?;
    set("org.gnome.system.proxy.https", "port", &http_port)?;
    set("org.gnome.system.proxy.socks", "host", "127.0.0.1")?;
    set("org.gnome.system.proxy.socks", "port", &socks_port)?;
    set("org.gnome.system.proxy.ftp", "host", "127.0.0.1")?;
    set("org.gnome.system.proxy.ftp", "port", &http_port)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn backup_path() -> PathBuf {
    super::database::data_directory()
        .join("runtime")
        .join(BACKUP_FILE)
}

#[cfg(target_os = "linux")]
fn save_backup(path: &PathBuf) -> Result<(), String> {
    let mut entries = Vec::with_capacity(SAVED_KEYS.len());
    for (schema, key) in SAVED_KEYS {
        entries.push(json!({
            "schema": schema,
            "key": key,
            "value": get(schema, key)?,
        }));
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the proxy backup directory: {error}"))?;
    }
    let contents = serde_json::to_vec_pretty(&Value::Array(entries))
        .map_err(|error| format!("Could not encode the saved system proxy settings: {error}"))?;
    fs::write(path, contents)
        .map_err(|error| format!("Could not save the current system proxy settings: {error}"))
}

#[cfg(target_os = "linux")]
fn restore() -> Result<(), String> {
    let path = backup_path();
    let Ok(contents) = fs::read(&path) else {
        return Ok(());
    };
    let entries = serde_json::from_slice::<Value>(&contents)
        .map_err(|error| format!("Could not read the saved system proxy settings: {error}"))?;

    let mut first_error = None;
    for entry in entries.as_array().into_iter().flatten() {
        let schema = entry
            .get("schema")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let key = entry.get("key").and_then(Value::as_str).unwrap_or_default();
        let value = entry
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Err(error) = set(schema, key, value) {
            first_error.get_or_insert(error);
        }
    }
    let _ = fs::remove_file(&path);

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(target_os = "linux")]
fn get(schema: &str, key: &str) -> Result<String, String> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .map_err(|error| format!("Could not run gsettings: {error}"))?;
    if !output.status.success() {
        return Err(command_error("gsettings get", schema, key, &output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn set(schema: &str, key: &str, value: &str) -> Result<(), String> {
    let output = Command::new("gsettings")
        .args(["set", schema, key, value])
        .output()
        .map_err(|error| format!("Could not run gsettings: {error}"))?;
    if !output.status.success() {
        return Err(command_error("gsettings set", schema, key, &output.stderr));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn command_error(command: &str, schema: &str, key: &str, stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        format!("{command} {schema} {key} failed.")
    } else {
        format!("{command} {schema} {key} failed: {detail}")
    }
}
