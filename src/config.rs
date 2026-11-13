use std::path::PathBuf;

use crate::{Result, SDCARD_ROOT};

/// Which `NextUI` releases the updater offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum VersionMode {
    /// Only regular releases (`GitHub` `prerelease == false`). Default.
    #[default]
    Stable,
    /// Regular releases plus prereleases (beta builds).
    Prerelease,
}

impl VersionMode {
    pub fn label(self) -> &'static str {
        match self {
            VersionMode::Stable => "稳定版",
            VersionMode::Prerelease => "预发布版",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            VersionMode::Stable => VersionMode::Prerelease,
            VersionMode::Prerelease => VersionMode::Stable,
        }
    }

    fn as_config_value(self) -> &'static str {
        match self {
            VersionMode::Stable => "stable",
            VersionMode::Prerelease => "prerelease",
        }
    }
}

const VERSION_MODE_KEY: &str = "version_mode";

/// `<SD>/.userdata/shared/nextui-updater.txt`, simple `key=value` lines.
pub fn config_path() -> PathBuf {
    PathBuf::from(&*SDCARD_ROOT).join(".userdata/shared/nextui-updater.txt")
}

pub fn load_version_mode() -> VersionMode {
    let Ok(content) = std::fs::read_to_string(config_path()) else {
        return VersionMode::default();
    };
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if key.trim() == VERSION_MODE_KEY {
                return match value.trim() {
                    "prerelease" => VersionMode::Prerelease,
                    _ => VersionMode::Stable,
                };
            }
        }
    }
    VersionMode::default()
}

pub fn save_version_mode(mode: VersionMode) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // keep any other keys that may be added later
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|l| {
            l.split_once('=')
                .is_none_or(|(k, _)| k.trim() != VERSION_MODE_KEY)
        })
        .filter(|l| !l.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect();
    lines.push(format!("{VERSION_MODE_KEY}={}", mode.as_config_value()));
    std::fs::write(&path, lines.join("\n") + "\n")?;
    Ok(())
}
