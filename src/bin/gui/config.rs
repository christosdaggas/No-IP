use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::keyring;

/// Persistent GUI configuration stored in XDG config dir.
///
/// **Secrets are not persisted to this file.** `password` and `api_key` live
/// only in memory at runtime; when the user opts into "Remember password" they
/// are stored in the platform keyring (Secret Service / Keychain / Credential
/// Manager) and re-loaded by [`AppConfig::load`].
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub username: String,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default)]
    pub hosts: BTreeMap<String, bool>,
    #[serde(default)]
    pub save_password: bool,

    // Runtime-only — never serialized to disk.
    #[serde(skip)]
    pub password: String,
    #[serde(skip)]
    pub api_key: String,

    // Migration: tolerate old on-disk fields so we can move them into the
    // keyring on first load. Never re-serialized (skip_serializing).
    #[serde(default, skip_serializing)]
    legacy_password: String,
    #[serde(default, skip_serializing)]
    legacy_api_key: String,
}

impl AppConfig {
    fn config_path() -> Option<PathBuf> {
        let mut p = dirs::config_dir()?;
        p.push("noip-duc");
        if let Err(e) = fs::create_dir_all(&p) {
            log::warn!("Failed to create config directory {}: {e}", p.display());
            return None;
        }
        p.push("config.json");
        Some(p)
    }

    pub fn load() -> Self {
        let Some(p) = Self::config_path() else {
            return Self::default();
        };
        let s = match fs::read_to_string(&p) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                log::warn!("Failed to read config {}: {e}", p.display());
                return Self::default();
            }
        };

        // Accept legacy field names so old configs are migrated transparently.
        let mut cfg: AppConfig = match serde_json::from_str(&s) {
            Ok(cfg) => cfg,
            Err(e) => {
                log::warn!("Failed to parse config {}: {e}", p.display());
                return Self::default();
            }
        };

        // Read legacy fields directly from JSON since serde renamed them.
        if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&s) {
            cfg.legacy_password = raw
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            cfg.legacy_api_key = raw
                .get("api_key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }

        // Migrate any plaintext secrets from the JSON into the keyring,
        // then rewrite the JSON without them.
        let migrated = cfg.migrate_legacy_secrets();

        // Pull the active password from the keyring if the user opted into
        // remembering it. (Migration just stored it there if needed.)
        if cfg.save_password && !cfg.username.is_empty() {
            if let Some(pw) = keyring::load(&cfg.username, keyring::Slot::Password) {
                cfg.password = pw;
            }
            if let Some(k) = keyring::load(&cfg.username, keyring::Slot::ApiKey) {
                cfg.api_key = k;
            }
        }

        if migrated {
            cfg.save();
        }

        cfg
    }

    /// Move any plaintext secrets that were on disk into the keyring. Returns
    /// true if anything moved (caller should re-write the config to drop them).
    fn migrate_legacy_secrets(&mut self) -> bool {
        let mut moved = false;
        if !self.legacy_password.is_empty() && !self.username.is_empty() {
            log::info!("Migrating plaintext password from config.json to keyring");
            if let Err(e) = keyring::store(
                &self.username,
                keyring::Slot::Password,
                &self.legacy_password,
            ) {
                log::warn!("Keyring store failed during migration: {e}; password kept in memory only");
            }
            self.password = std::mem::take(&mut self.legacy_password);
            moved = true;
        }
        if !self.legacy_api_key.is_empty() && !self.username.is_empty() {
            log::info!("Migrating plaintext API key from config.json to keyring");
            if let Err(e) = keyring::store(
                &self.username,
                keyring::Slot::ApiKey,
                &self.legacy_api_key,
            ) {
                log::warn!("Keyring store failed during migration: {e}; api key kept in memory only");
            }
            self.api_key = std::mem::take(&mut self.legacy_api_key);
            moved = true;
        }
        moved
    }

    pub fn save(&self) {
        let Some(p) = Self::config_path() else { return };
        let s = match serde_json::to_string_pretty(self) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Failed to serialize config: {e}");
                return;
            }
        };
        if let Err(e) = fs::write(&p, &s) {
            log::warn!("Failed to save config {}: {e}", p.display());
            return;
        }
        // Defence-in-depth even though no secrets are written.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = fs::set_permissions(&p, fs::Permissions::from_mode(0o600)) {
                log::warn!("Failed to chmod 0600 on config {}: {e}", p.display());
            }
        }
    }

    /// Persist the password+api_key for `username` into the keyring. Called
    /// when the user submits the login form with "Remember password" checked.
    pub fn remember_secrets(&self) {
        if self.username.is_empty() {
            return;
        }
        if !self.password.is_empty() {
            if let Err(e) = keyring::store(&self.username, keyring::Slot::Password, &self.password) {
                log::warn!("Failed to store password in keyring: {e}");
            }
        }
        if !self.api_key.is_empty() {
            if let Err(e) = keyring::store(&self.username, keyring::Slot::ApiKey, &self.api_key) {
                log::warn!("Failed to store API key in keyring: {e}");
            }
        }
    }

    /// Drop any keyring entries for the current `username`.
    pub fn forget_secrets(&self) {
        if self.username.is_empty() {
            return;
        }
        keyring::delete(&self.username, keyring::Slot::Password);
        keyring::delete(&self.username, keyring::Slot::ApiKey);
    }

    pub fn selected_hosts(&self) -> Vec<String> {
        self.hosts
            .iter()
            .filter_map(|(h, &on)| if on { Some(h.clone()) } else { None })
            .collect()
    }
}
