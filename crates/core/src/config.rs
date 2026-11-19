//! Settings and credential storage.
//!
//! Tokens live in the OS keychain (Keychain / Credential Manager / Secret
//! Service). Environment variables `MODRINTH_TOKEN` and `CURSEFORGE_API_KEY`
//! always take precedence, which is handy for CI.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
