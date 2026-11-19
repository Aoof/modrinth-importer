//! Settings and credential storage.
//!
//! Tokens live in the OS keychain (Keychain / Credential Manager / Secret
//! Service). Environment variables `MODRINTH_TOKEN` and `CURSEFORGE_API_KEY`
//! always take precedence, which is handy for CI.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SERVICE: &str = "modrinth-importer";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Secret {
    ModrinthToken,
    CurseForgeKey,
}

impl Secret {
    fn account(self) -> &'static str {
        match self {
            Secret::ModrinthToken => "modrinth-token",
            Secret::CurseForgeKey => "curseforge-api-key",
        }
    }
    fn env(self) -> &'static str {
        match self {
            Secret::ModrinthToken => "MODRINTH_TOKEN",
            Secret::CurseForgeKey => "CURSEFORGE_API_KEY",
        }
    }
}

pub fn get_secret(s: Secret) -> Option<String> {
    if let Ok(v) = std::env::var(s.env()) {
        if !v.trim().is_empty() {
            return Some(v);
        }
    }
    keyring::Entry::new(SERVICE, s.account())
        .ok()?
        .get_password()
        .ok()
}

/// Like [`get_secret`], also reporting where the value came from.
pub fn secret_source(s: Secret) -> Option<(String, &'static str)> {
    if let Ok(v) = std::env::var(s.env()) {
        if !v.trim().is_empty() {
            return Some((v, "environment variable"));
        }
    }
    keyring::Entry::new(SERVICE, s.account())
        .ok()?
        .get_password()
        .ok()
        .map(|v| (v, "OS keychain"))
}

pub fn set_secret(s: Secret, value: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, s.account())
        .and_then(|e| e.set_password(value))
        .map_err(|e| Error::Invalid(format!("could not store credential in keychain: {e}")))
}

pub fn delete_secret(s: Secret) -> Result<()> {
    match keyring::Entry::new(SERVICE, s.account()).and_then(|e| e.delete_credential()) {
        Ok(()) => Ok(()),
        Err(e) => Err(Error::Invalid(format!("could not remove credential: {e}"))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Override for the Modrinth App data directory.
    pub modrinth_app_dir: Option<PathBuf>,
    /// Bundle unmatched mods as overrides when a jar is available.
    #[serde(default = "yes")]
    pub bundle_unmatched: bool,
}

fn yes() -> bool {
    true
}

pub fn settings_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "Aoof", "ModrinthImporter")
        .map(|d| d.config_dir().join("settings.json"))
}

pub fn load_settings() -> Settings {
    settings_path()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            modrinth_app_dir: None,
            bundle_unmatched: true,
        }
    }
}

pub fn save_settings(s: &Settings) -> Result<()> {
    let p =
        settings_path().ok_or_else(|| Error::Invalid("no config directory available".into()))?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, serde_json::to_vec_pretty(s)?)?;
    Ok(())
}

/// Default Modrinth App data directory for the current OS.
pub fn default_modrinth_app_dir() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    // Modrinth App stores data in a "ModrinthApp" folder under the OS app-data dir.
    Some(base.data_dir().join("ModrinthApp"))
}

/// Bundle identifier the desktop app uses for its own data directories.
const APP_ID: &str = "com.aoof.modrinthimporter";

fn is_ours(p: &Path) -> bool {
    p.file_name()
        .map(|n| {
            n.to_string_lossy()
                .to_ascii_lowercase()
                .contains("modrinthimporter")
        })
        .unwrap_or(false)
        && p.components().count() >= 3
}

/// Every file or directory this tool may have created outside its install
/// location (settings, caches, webview data, saved state) that exists now.
pub fn local_data_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    if let Some(p) = directories::ProjectDirs::from("com", "Aoof", "ModrinthImporter") {
        v.extend(
            [
                p.config_dir(),
                p.data_dir(),
                p.data_local_dir(),
                p.cache_dir(),
            ]
            .map(Path::to_path_buf),
        );
    }
    if let Some(b) = directories::BaseDirs::new() {
        for d in [
            b.config_dir(),
            b.data_dir(),
            b.data_local_dir(),
            b.cache_dir(),
        ] {
            v.push(d.join(APP_ID));
        }
        if cfg!(target_os = "macos") {
            let lib = b.home_dir().join("Library");
            for d in ["WebKit", "HTTPStorages", "Logs", "Caches"] {
                v.push(lib.join(d).join(APP_ID));
            }
            v.push(lib.join("Preferences").join(format!("{APP_ID}.plist")));
            v.push(
                lib.join("Saved Application State")
                    .join(format!("{APP_ID}.savedState")),
            );
        }
    }
    v.sort();
    v.dedup();
    // macOS is case-insensitive by default: skip entries that are the same on disk.
    v.retain(|p| p.exists() && is_ours(p));
    let mut seen = std::collections::HashSet::new();
    v.retain(|p| {
        std::fs::canonicalize(p)
            .map(|c| seen.insert(c))
            .unwrap_or(true)
    });
    v
}

/// Remove everything [`local_data_paths`] lists, plus stored credentials.
/// Returns the paths that were removed.
pub fn purge_local_data() -> Result<Vec<PathBuf>> {
    let mut removed = vec![];
    for p in local_data_paths() {
        if !p.exists() {
            continue; // already gone (case-insensitive duplicate)
        }
        if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else {
            std::fs::remove_file(&p)?;
        }
        // Tidy an empty vendor folder such as `%APPDATA%\Aoof` (never removes non-empty dirs).
        if let Some(parent) = p.parent() {
            if parent.file_name().is_some_and(|n| n == "Aoof") {
                let _ = std::fs::remove_dir(parent);
            }
        }
        removed.push(p);
    }
    Ok(removed)
}

/// Delete both stored credentials; returns how many existed.
pub fn purge_secrets() -> usize {
    [Secret::ModrinthToken, Secret::CurseForgeKey]
        .into_iter()
        .filter(|s| {
            keyring::Entry::new(SERVICE, s.account())
                .map(|e| e.delete_credential().is_ok())
                .unwrap_or(false)
        })
        .count()
}
