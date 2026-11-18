use crate::theme::ThemeId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub theme: ThemeId,
    pub custom_hue: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeId::Neon,
            custom_hue: 186.0,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("arch-control-center/config.toml");
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".config/arch-control-center/config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match Self::parse(&text) {
                Ok(cfg) => cfg,
                Err(err) => {
                    log::warn!("config {}: {err}", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut cfg: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        cfg.custom_hue = cfg.custom_hue.rem_euclid(360.0);
        Ok(cfg)
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(dir) {
                log::warn!("config dir: {err}");
                return;
            }
        }
        match toml::to_string_pretty(self) {
            Ok(text) => {
                if let Err(err) = std::fs::write(&path, text) {
                    log::warn!("config write {}: {err}", path.display());
                }
            }
            Err(err) => log::warn!("config encode: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_theme() {
        let cfg = Config::parse("theme = \"acid\"\ncustom_hue = 400\n").unwrap();
        assert_eq!(cfg.theme, ThemeId::Acid);
        assert!((cfg.custom_hue - 40.0).abs() < 0.01);
        let text = toml::to_string(&cfg).unwrap();
        let again = Config::parse(&text).unwrap();
        assert_eq!(again.theme, ThemeId::Acid);
    }
}
