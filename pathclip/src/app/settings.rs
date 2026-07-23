use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use global_hotkey::hotkey::HotKey;
use regex::Regex;
use serde::Deserialize;
use tracing::info;

const DEFAULT_SETTINGS: &str = include_str!("../../pathclip.toml.example");

#[derive(Debug)]
pub struct Settings {
    auto_profile: Option<String>,
    profiles: BTreeMap<String, Profile>,
    hotkey_profiles: HashMap<u32, String>,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub hotkey: Option<HotKey>,
    pub steps: Vec<TransformStep>,
}

#[derive(Debug, Clone)]
pub enum TransformStep {
    Regex {
        regex: Regex,
        replacement: String,
    },
    ForwardSlash,
    Wsl,
    FileUri,
}

#[derive(Debug, Deserialize)]
struct SettingsFile {
    #[serde(default)]
    auto_profile: String,
    profiles: BTreeMap<String, ProfileFile>,
}

#[derive(Debug, Deserialize)]
struct ProfileFile {
    #[serde(default)]
    hotkey: String,
    steps: Vec<TransformStepFile>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum TransformStepFile {
    Regex {
        pattern: String,
        replacement: String,
    },
    ForwardSlash,
    Wsl,
    FileUri,
}

impl Settings {
    pub fn load(explicit_path: Option<PathBuf>) -> Result<Self> {
        let path = resolve_settings_path(explicit_path)?;
        let Some(path) = path else {
            info!("settings file was not found, using built-in defaults");
            return Self::parse(DEFAULT_SETTINGS);
        };

        let source = fs::read_to_string(&path)
            .with_context(|| format!("failed to read settings file: {}", path.display()))?;
        let settings = Self::parse(&source)
            .with_context(|| format!("invalid settings file: {}", path.display()))?;
        info!(path = %path.display(), "settings loaded");
        Ok(settings)
    }

    pub fn default_source() -> &'static str {
        DEFAULT_SETTINGS
    }

    pub fn auto_profile(&self) -> Option<&Profile> {
        self.auto_profile
            .as_ref()
            .and_then(|name| self.profiles.get(name))
    }

    pub fn profile_for_hotkey(&self, hotkey_id: u32) -> Option<&Profile> {
        self.hotkey_profiles
            .get(&hotkey_id)
            .and_then(|name| self.profiles.get(name))
    }

    pub fn registered_hotkeys(&self) -> Vec<HotKey> {
        self.profiles
            .values()
            .filter_map(|profile| profile.hotkey)
            .collect()
    }

    pub(super) fn parse(source: &str) -> Result<Self> {
        let file: SettingsFile = toml::from_str(source).context("failed to parse TOML")?;
        if file.profiles.is_empty() {
            bail!("at least one profile is required");
        }

        let mut profiles = BTreeMap::new();
        let mut hotkey_profiles = HashMap::new();

        for (name, profile_file) in file.profiles {
            if profile_file.steps.is_empty() {
                bail!("profile `{name}` must contain at least one step");
            }

            let hotkey = parse_hotkey(&name, &profile_file.hotkey)?;
            if let Some(hotkey) = hotkey
                && let Some(existing) = hotkey_profiles.insert(hotkey.id(), name.clone())
            {
                    bail!(
                        "profiles `{existing}` and `{name}` use the same hotkey `{hotkey}`"
                    );
            }

            let steps = profile_file
                .steps
                .into_iter()
                .map(|step| compile_step(&name, step))
                .collect::<Result<Vec<_>>>()?;

            profiles.insert(
                name.clone(),
                Profile {
                    name,
                    hotkey,
                    steps,
                },
            );
        }

        let auto_profile = match file.auto_profile.trim() {
            "" => None,
            name if profiles.contains_key(name) => Some(name.to_string()),
            name => bail!("auto_profile references unknown profile `{name}`"),
        };

        Ok(Self {
            auto_profile,
            profiles,
            hotkey_profiles,
        })
    }
}

fn resolve_settings_path(explicit_path: Option<PathBuf>) -> Result<Option<PathBuf>> {
    let requested = explicit_path
        .or_else(|| env::var_os("PATHCLIP_CONFIG").filter(|value| !value.is_empty()).map(PathBuf::from));

    if let Some(path) = requested {
        if !path.is_file() {
            info!(path = %path.display(), "settings file was not found, using built-in defaults");
            return Ok(None);
        }
        return Ok(Some(path));
    }

    let home = dirs_next::home_dir().context("failed to determine the user home directory")?;
    let path = home.join(".config").join("pathclip").join("config.toml");
    Ok(path.is_file().then_some(path))
}

fn parse_hotkey(profile_name: &str, source: &str) -> Result<Option<HotKey>> {
    let source = source.trim();
    if source.is_empty() {
        return Ok(None);
    }

    source
        .parse::<HotKey>()
        .map(Some)
        .with_context(|| format!("profile `{profile_name}` has invalid hotkey `{source}`"))
}

fn compile_step(profile_name: &str, step: TransformStepFile) -> Result<TransformStep> {
    match step {
        TransformStepFile::Regex {
            pattern,
            replacement,
        } => {
            let regex = Regex::new(&pattern).with_context(|| {
                format!("profile `{profile_name}` has invalid regex `{pattern}`")
            })?;
            Ok(TransformStep::Regex { regex, replacement })
        }
        TransformStepFile::ForwardSlash => Ok(TransformStep::ForwardSlash),
        TransformStepFile::Wsl => Ok(TransformStep::Wsl),
        TransformStepFile::FileUri => Ok(TransformStep::FileUri),
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn default_settings_are_valid() {
        let settings = Settings::parse(Settings::default_source()).unwrap();
        assert_eq!(settings.auto_profile().unwrap().name, "slash");
        assert!(settings.registered_hotkeys().is_empty());
    }

    #[test]
    fn empty_auto_profile_and_hotkey_are_allowed() {
        let settings = Settings::parse(
            r#"
                auto_profile = ""

                [profiles.slash]
                hotkey = ""
                steps = [{ type = "forward-slash" }]
            "#,
        )
        .unwrap();

        assert!(settings.auto_profile().is_none());
        assert!(settings.registered_hotkeys().is_empty());
    }

    #[test]
    fn unknown_auto_profile_is_rejected() {
        let error = Settings::parse(
            r#"
                auto_profile = "missing"

                [profiles.slash]
                steps = [{ type = "forward-slash" }]
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown profile"));
    }

    #[test]
    fn invalid_regex_is_rejected() {
        let error = Settings::parse(
            r#"
                auto_profile = "slash"

                [profiles.slash]
                steps = [{ type = "regex", pattern = "(", replacement = "$0" }]
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("invalid regex"));
    }

    #[test]
    fn duplicate_hotkeys_are_rejected() {
        let error = Settings::parse(
            r#"
                auto_profile = "a"

                [profiles.a]
                hotkey = "Ctrl+Shift+V"
                steps = [{ type = "forward-slash" }]

                [profiles.b]
                hotkey = "Ctrl+Shift+V"
                steps = [{ type = "wsl" }]
            "#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("same hotkey"));
    }
}
