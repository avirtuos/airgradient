use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::models::{GraphConfig, SensorConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            data_dir: default_data_dir(),
        }
    }
}

fn default_listen_addr() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("./data")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub graphs: GraphConfig,
    #[serde(default)]
    pub sensors: Vec<SensorConfig>,
}

impl AppConfig {
    /// Load config from a TOML file. Creates a default config file if it doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            let default = AppConfig::default();
            default.save(path)?;
            info!("Created default config at {}", path.display());
            return Ok(default);
        }

        let contents = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Persist the current config to a TOML file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}

/// Resolve the config file path: CLI arg > env var > default
pub fn resolve_config_path(cli_path: Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_path {
        return p;
    }
    if let Ok(env_path) = std::env::var("AIRGRADIENT_CONFIG") {
        return PathBuf::from(env_path);
    }
    PathBuf::from("./config.toml")
}
