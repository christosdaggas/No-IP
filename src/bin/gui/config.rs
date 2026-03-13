use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Persistent GUI configuration stored in XDG config dir.
#[derive(Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default)]
    pub hosts: BTreeMap<String, bool>,
    #[serde(default)]
    pub save_password: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            api_key: String::new(),
            dark_mode: true,
            hosts: BTreeMap::new(),
            save_password: false,
        }
    }
}

impl AppConfig {
    fn config_path() -> Option<PathBuf> {
        let mut p = dirs::config_dir()?;
        p.push("noip-duc");
        let _ = fs::create_dir_all(&p);
        p.push("config.json");
        Some(p)
    }

    pub fn load() -> Self {
        Self::config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::config_path() else { return };
        let Ok(s) = serde_json::to_string_pretty(self) else { return };
        if let Err(e) = fs::write(&p, &s) {
            log::warn!("Failed to save config: {e}");
            return;
        }
        // Restrict permissions so the password isn't world-readable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
        }
    }

    pub fn selected_hosts(&self) -> Vec<String> {
        self.hosts
            .iter()
            .filter_map(|(h, &on)| if on { Some(h.clone()) } else { None })
            .collect()
    }
}
