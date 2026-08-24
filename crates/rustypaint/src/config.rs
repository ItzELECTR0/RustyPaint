use crate::canvas::NewCanvas;
use crate::ui::theme::{Choice, Scheme};

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: Choice,
    pub accent: Scheme,
    pub new_canvas: NewCanvas,
    pub acrylic: bool,
    pub decorations: bool,
    pub custom_colours: Vec<[u8; 4]>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Choice::default(),
            accent: Scheme::default(),
            new_canvas: NewCanvas::default(),
            acrylic: true,
            decorations: false,
            custom_colours: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> (Self, Option<String>) {
        let Some(path) = path() else {
            return (Self::default(), None);
        };
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None);
            }
            Err(e) => {
                return (Self::default(), Some(format!("Cannot read settings: {e}")));
            }
        };
        match toml::from_str(&contents) {
            Ok(config) => (config, None),
            Err(e) => (
                Self::default(),
                Some(format!(
                    "Settings file is not readable, using defaults: {}",
                    first_line(&e)
                )),
            ),
        }
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

pub fn boot() -> &'static (Config, Option<String>) {
    static BOOT: std::sync::OnceLock<(Config, Option<String>)> = std::sync::OnceLock::new();
    BOOT.get_or_init(Config::load)
}

pub fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("config.toml"))
}

#[cfg(target_os = "windows")]
fn config_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("RustyPaint"))
}

#[cfg(target_os = "macos")]
fn config_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("HOME")?)
            .join("Library")
            .join("Application Support")
            .join("RustyPaint"),
    )
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn config_dir() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))?;
    Some(root.join("rustypaint"))
}

fn first_line(error: &toml::de::Error) -> String {
    error
        .to_string()
        .lines()
        .next()
        .unwrap_or("malformed")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_survives_a_round_trip() {
        let config = Config {
            theme: Choice::Dark,
            accent: Scheme::Classic,
            new_canvas: NewCanvas::Fixed(1920, 1080),
            acrylic: false,
            decorations: true,
            custom_colours: vec![[254, 168, 69, 255]],
        };
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }

    #[test]
    fn a_file_from_an_older_build_still_loads() {
        let config: Config = toml::from_str("theme = \"dark\"\n").unwrap();
        assert_eq!(config.theme, Choice::Dark);
        assert_eq!(config.accent, Scheme::Rusty);
        assert!(config.acrylic);
    }

    #[test]
    fn a_key_this_build_has_never_heard_of_is_ignored() {
        let config: Config = toml::from_str("quantum_brush = \"on\"\n").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        assert_eq!(toml::from_str::<Config>("").unwrap(), Config::default());
    }

    #[test]
    fn the_enums_write_as_words_rather_than_numbers() {
        let text = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(text.contains("theme = \"auto\""), "{text}");
        assert!(text.contains("accent = \"rusty\""), "{text}");
    }

    #[test]
    fn nonsense_does_not_load_but_does_not_panic_either() {
        assert!(toml::from_str::<Config>("theme = \"purple\"").is_err());
        assert!(toml::from_str::<Config>("this is not toml at all {{").is_err());
    }
}
