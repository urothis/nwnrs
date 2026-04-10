use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use bevy::prelude::{App, Plugin, Resource, Startup};
use nwnrs_game::prelude::{
    DEFAULT_KEYFILES, find_nwnrs_root, find_user_root, new_default_resman, resolve_language_root,
};
use nwnrs_resman::ResMan;
use tracing::{error, info};

use crate::install_state::set_shared_resman;

/// Configuration for Bevy-side NWN install and KEY loading.
#[derive(Debug, Clone, Resource)]
pub struct NwnInstallSettings {
    /// Optional NWN install-root override passed through to `nwnrs-game`.
    pub root_override:  String,
    /// Optional NWN user-directory override passed through to `nwnrs-game`.
    pub user_override:  String,
    /// Language folder under `lang/`.
    pub language:       String,
    /// ResMan cache size.
    pub cache_size:     usize,
    /// Whether to load KEY/BIF containers.
    pub load_keys:      bool,
    /// Whether to load override directories.
    pub load_overrides: bool,
    /// Explicit key basenames, or empty for the default load order.
    pub keys:           Vec<String>,
}

impl Default for NwnInstallSettings {
    fn default() -> Self {
        Self {
            root_override:  String::new(),
            user_override:  String::new(),
            language:       "english".to_string(),
            cache_size:     0,
            load_keys:      true,
            load_overrides: false,
            keys:           Vec::new(),
        }
    }
}

/// The discovered NWN install and loaded resource manager.
#[derive(Resource)]
pub struct NwnInstall {
    /// Resolved NWN install root.
    pub root:          PathBuf,
    /// Resolved NWN user directory.
    pub user_root:     PathBuf,
    /// Resolved language root.
    pub language_root: PathBuf,
    /// KEY paths found under the install.
    pub key_paths:     Vec<PathBuf>,
    /// Loaded NWN resource manager.
    pub resman:        Arc<Mutex<ResMan>>,
}

/// Bevy plugin that discovers an NWN install, loads KEYs, and stores the
/// resulting `ResMan` as a Bevy resource.
#[derive(Debug, Clone)]
pub struct NwnInstallPlugin {
    settings: NwnInstallSettings,
}

impl NwnInstallPlugin {
    /// Creates the plugin with explicit settings.
    pub fn new(settings: NwnInstallSettings) -> Self {
        Self {
            settings,
        }
    }
}

impl Default for NwnInstallPlugin {
    fn default() -> Self {
        Self::new(NwnInstallSettings::default())
    }
}

impl Plugin for NwnInstallPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .add_systems(Startup, load_nwn_install);
    }
}

fn load_nwn_install(
    mut commands: bevy::prelude::Commands<'_, '_>,
    settings: bevy::prelude::Res<'_, NwnInstallSettings>,
) {
    let root = match find_nwnrs_root(&settings.root_override) {
        Ok(root) => root,
        Err(error) => {
            error!("failed to resolve NWN root: {error}");
            return;
        }
    };
    let user_root = match find_user_root(&settings.user_override) {
        Ok(user_root) => user_root,
        Err(error) => {
            error!("failed to resolve NWN user directory: {error}");
            return;
        }
    };
    let language_root = match resolve_language_root(&root, &settings.language) {
        Ok(language_root) => language_root,
        Err(error) => {
            error!("failed to resolve NWN language directory: {error}");
            return;
        }
    };
    let key_names = resolved_key_names(&settings.keys);
    let key_paths = resolve_key_paths(&root, &language_root, &key_names);
    let resman = match new_default_resman(
        &root,
        &user_root,
        &settings.language,
        settings.cache_size,
        settings.load_keys,
        settings.load_overrides,
        &key_names,
        &[],
        &[],
        &[],
    ) {
        Ok(resman) => resman,
        Err(error) => {
            error!("failed to build NWN resource manager: {error}");
            return;
        }
    };
    let resman = Arc::new(Mutex::new(resman));

    info!(path = %root.display(), "resolved NWN root");
    info!(path = %user_root.display(), "resolved NWN user directory");
    for key_path in &key_paths {
        info!(path = %key_path.display(), "loaded KEY");
    }

    set_shared_resman(Arc::clone(&resman));

    commands.insert_resource(NwnInstall {
        language_root,
        root,
        user_root,
        key_paths,
        resman,
    });
}

fn resolved_key_names(keys: &[String]) -> Vec<String> {
    if keys.is_empty() {
        DEFAULT_KEYFILES
            .iter()
            .map(|key| (*key).to_string())
            .collect()
    } else {
        keys.to_vec()
    }
}

fn resolve_key_paths(root: &Path, language_root: &Path, keys: &[String]) -> Vec<PathBuf> {
    keys.iter()
        .filter_map(|key| {
            let relative = Path::new("data").join(format!("{key}.key"));
            let language_candidate = language_root.join(&relative);
            if language_candidate.is_file() {
                Some(language_candidate)
            } else {
                let root_candidate = root.join(relative);
                root_candidate.is_file().then_some(root_candidate)
            }
        })
        .collect()
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use super::{NwnInstallSettings, resolve_key_paths, resolved_key_names};

    #[test]
    fn default_install_settings_match_expected_language_and_key_behavior() {
        let settings = NwnInstallSettings::default();
        assert_eq!(settings.language, "english");
        assert!(settings.load_keys);
        assert!(settings.keys.is_empty());
    }

    #[test]
    fn empty_key_list_expands_to_default_keyfiles() {
        let resolved = resolved_key_names(&[]);
        assert_eq!(resolved.len(), 4);
        assert!(resolved.iter().any(|key| key == "nwn_base"));
    }

    #[test]
    fn missing_key_files_produce_no_paths() {
        let root = std::env::temp_dir().join("nwnrs-bevy-missing-key-paths");
        let language_root = root.join("lang").join("en");
        let key_paths = resolve_key_paths(&root, &language_root, &resolved_key_names(&[]));
        assert!(key_paths.is_empty());
    }
}
