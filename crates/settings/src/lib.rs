use std::path::PathBuf;

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default)]
    pub realclose: bool,
    #[serde(default)]
    pub startminimized: bool,
    #[serde(default)]
    pub visibility: u8,
    #[serde(default)]
    pub port: Option<u32>,
    #[serde(default)]
    pub download_path: Option<PathBuf>,
    #[serde(default)]
    pub debug_level: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autostart: true,
            realclose: false,
            startminimized: false,
            visibility: 0,
            port: None,
            download_path: None,
            debug_level: None,
        }
    }
}

impl Settings {
    pub fn config_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("", "", "rquickshare-gpui")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        Ok(dirs.config_dir().join("settings.json"))
    }

    pub fn load() -> Self {
        let path = match Self::config_path() {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Could not determine config path: {e}");
                return Self::default();
            }
        };

        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                log::warn!("Could not parse settings: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        log::debug!("Settings saved to {}", path.display());
        Ok(())
    }
}
