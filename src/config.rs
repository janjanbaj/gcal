use crate::{Result, logging::get_app_path};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub client_file: Option<PathBuf>,
    pub calendar_ids: Vec<String>,
    pub use_24h_clock: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            client_file: None,
            calendar_ids: vec!["primary".to_string()],
            use_24h_clock: Some(true),
        }
    }
}

impl Config {
    pub fn new() -> Result<Config> {
        let app_paths = get_app_path();
        let config_file = app_paths.config_dir.join("config.toml");

        if !config_file.exists() {
            if let Some(parent) = config_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let default_config = Config::default();
            let toml_string = toml::to_string_pretty(&default_config)
                .expect("ERROR: Unable to convert Config struct to toml string");

            std::fs::write(&config_file, toml_string)?;
            eprintln!("Auto generated missing config file: {:?}", &config_file);
            return Ok(default_config);
        }

        let settings = config::Config::builder()
            .add_source(config::File::from(config_file))
            .build()?;
        let mut config: Config = settings.try_deserialize()?;

        if config.calendar_ids.is_empty() {
            config.calendar_ids = vec!["primary".to_string()];
        }

        config.client_file = match config.client_file {
            Some(path) if !path.as_os_str().is_empty() => Some(expand_filepath(&path)?),
            _ => None,
        };

        Ok(config)
    }
}

fn expand_filepath(path: &Path) -> Result<PathBuf> {
    let path_str = path.to_string_lossy();
    let expanded_path_str = shellexpand::full(&path_str)?;
    Ok(PathBuf::from(expanded_path_str.as_ref()))
}
