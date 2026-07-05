//! Per-app dictation profiles: which polish level and tone a given app gets.
//!
//! Resolution order for a target app: an explicit user pin wins, then a
//! built-in category default (email → polished + formal), then the global
//! dictation settings. Pins are persisted as JSON keyed by bundle id — this is
//! the "knows your world" behavior the pill surfaces.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::settings::DictationSettings;
use crate::cleanup::{DictationStyle, Polish};
use crate::paste;

/// The polish level and writing tone for one app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProfile {
    pub polish: Polish,
    pub style: DictationStyle,
}

/// Pinned per-app profiles, keyed by macOS bundle id.
pub type Overrides = HashMap<String, AppProfile>;

/// Resolve the effective profile for a target app.
pub fn resolve(
    bundle_id: Option<&str>,
    global: &DictationSettings,
    pinned: &Overrides,
) -> AppProfile {
    if let Some(profile) = bundle_id.and_then(|id| pinned.get(id)) {
        return *profile;
    }
    if paste::is_email_app(bundle_id) {
        return AppProfile {
            polish: Polish::Polished,
            style: DictationStyle::Formal,
        };
    }
    AppProfile {
        polish: global.polish,
        style: global.style,
    }
}

fn profiles_path(config_dir: &Path) -> PathBuf {
    config_dir.join("dictation-app-profiles.json")
}

pub fn load(config_dir: &Path) -> Overrides {
    std::fs::read_to_string(profiles_path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, overrides: &Overrides) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let raw = serde_json::to_string_pretty(overrides).expect("profiles serialize");
    std::fs::write(profiles_path(config_dir), raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global() -> DictationSettings {
        DictationSettings {
            polish: Polish::Clean,
            style: DictationStyle::Standard,
            ..Default::default()
        }
    }

    #[test]
    fn pin_wins_over_the_email_default() {
        let mut pinned = Overrides::new();
        pinned.insert(
            "com.apple.mail".into(),
            AppProfile {
                polish: Polish::Raw,
                style: DictationStyle::CasualLowercase,
            },
        );
        let p = resolve(Some("com.apple.mail"), &global(), &pinned);
        assert_eq!(p.polish, Polish::Raw);
        assert_eq!(p.style, DictationStyle::CasualLowercase);
    }

    #[test]
    fn email_gets_formal_polished_by_default() {
        let p = resolve(Some("com.apple.mail"), &global(), &Overrides::new());
        assert_eq!(p.polish, Polish::Polished);
        assert_eq!(p.style, DictationStyle::Formal);
    }

    #[test]
    fn unknown_app_falls_back_to_global() {
        let p = resolve(Some("com.apple.Safari"), &global(), &Overrides::new());
        assert_eq!(p.polish, Polish::Clean);
        assert_eq!(p.style, DictationStyle::Standard);
    }

    #[test]
    fn no_bundle_falls_back_to_global() {
        let p = resolve(None, &global(), &Overrides::new());
        assert_eq!(p.polish, Polish::Clean);
    }

    #[test]
    fn round_trips_to_disk() {
        let dir = std::env::temp_dir().join(format!("arya-profiles-{}", uuid::Uuid::new_v4()));
        let mut o = Overrides::new();
        o.insert(
            "com.tinyspeck.slackmacgap".into(),
            AppProfile {
                polish: Polish::Clean,
                style: DictationStyle::CasualLowercase,
            },
        );
        save(&dir, &o).unwrap();
        let loaded = load(&dir);
        assert_eq!(
            loaded.get("com.tinyspeck.slackmacgap").unwrap().polish,
            Polish::Clean
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
