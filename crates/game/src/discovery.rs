use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tracing::{debug, info, instrument, warn};

use crate::{GameError, GameResult, Platform};

#[derive(Debug, Deserialize)]
struct BeamdogSettings {
    folders: Option<Vec<String>>,
}

/// Locates the NWN user directory.
///
/// Resolution order is: explicit override, `nwnrs_HOME`,
/// `nwnrs_USER_DIRECTORY`, then the platform-specific default location.
#[instrument(level = "info", skip_all, err, fields(override_dir))]
pub fn find_user_root(override_dir: &str) -> GameResult<PathBuf> {
    find_user_root_impl(
        override_dir,
        |key| std::env::var(key).ok(),
        current_home_dir,
        current_platform(),
    )
}

/// Locates the NWN installation root.
///
/// Resolution order is: explicit override, `nwnrs_ROOT`, Steam install
/// heuristics, then Beamdog client settings heuristics.
#[instrument(level = "info", skip_all, err, fields(override_dir))]
pub fn find_nwnrs_root(override_dir: &str) -> GameResult<PathBuf> {
    find_nwnrs_root_impl(
        override_dir,
        |key| std::env::var(key).ok(),
        current_home_dir,
        current_platform(),
    )
}

/// Resolves an NWN language folder under `root/lang`, accepting both common
/// long-form names such as `english` and short on-disk codes such as `en`.
#[instrument(level = "info", skip_all, err, fields(root = %root.as_ref().display(), language))]
pub fn resolve_language_root(root: impl AsRef<Path>, language: &str) -> GameResult<PathBuf> {
    let root = root.as_ref();
    let language_root = root.join("lang");
    let requested = language_root.join(language);
    if requested.is_dir() {
        return Ok(requested);
    }

    for alias in language_aliases(language) {
        let candidate = language_root.join(alias);
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    Err(GameError::msg(format!(
        "language {} not found",
        requested.display()
    )))
}

#[instrument(
    level = "info",
    skip(env_get, home_dir),
    err,
    fields(override_dir, platform = ?platform)
)]
pub(crate) fn find_user_root_impl<E, H>(
    override_dir: &str,
    env_get: E,
    home_dir: H,
    platform: Platform,
) -> GameResult<PathBuf>
where
    E: Fn(&str) -> Option<String>,
    H: Fn() -> Option<PathBuf>,
{
    debug!("resolving user root");
    let result = first_nonempty_path(
        override_dir,
        env_get("nwnrs_HOME").as_deref(),
        env_get("nwnrs_USER_DIRECTORY").as_deref(),
        match platform {
            Platform::MacOs => {
                home_dir().map(|home| home.join("Documents").join("Neverwinter Nights"))
            }
            Platform::Linux => {
                home_dir().map(|home| home.join(".local").join("share").join("Neverwinter Nights"))
            }
            Platform::Windows => {
                home_dir().map(|home| home.join("Documents").join("Neverwinter Nights"))
            }
        },
    );

    match result {
        Some(path) if path.is_dir() => {
            info!(path = %path.display(), "resolved user root");
            Ok(path)
        }
        _ => Err(GameError::msg(
            "Could not locate NWN user directory; try --userdirectory or set nwnrs_HOME \
             (nwnrs_USER_DIRECTORY also works, but is considered alternate)",
        )),
    }
}

#[instrument(
    level = "info",
    skip(env_get, home_dir),
    err,
    fields(override_dir, platform = ?platform)
)]
pub(crate) fn find_nwnrs_root_impl<E, H>(
    override_dir: &str,
    env_get: E,
    home_dir: H,
    platform: Platform,
) -> GameResult<PathBuf>
where
    E: Fn(&str) -> Option<String>,
    H: Fn() -> Option<PathBuf>,
{
    debug!("resolving game root");
    let mut result =
        first_nonempty_path(override_dir, env_get("nwnrs_ROOT").as_deref(), None, None);

    if result.is_none() {
        let steamapps = steamapps_dir(platform, &home_dir);
        let candidate = steamapps.join("Neverwinter Nights");
        if candidate.join("data").is_dir() && candidate.join("steam_appid.txt").is_file() {
            debug!(path = %candidate.display(), "resolved game root from steam installation");
            result = Some(candidate);
        }
    }

    if result.is_none() {
        let settings_file = beamdog_settings_path(platform, &home_dir);
        if settings_file.is_file() {
            let data = fs::read_to_string(&settings_file)?;
            let settings: BeamdogSettings = serde_json::from_str(&data)
                .map_err(|_error| GameError::msg("Beamdog settings missing folders array"))?;
            let folders = settings
                .folders
                .as_deref()
                .ok_or_else(|| GameError::msg("Beamdog settings missing folders array"))?;

            for torrent_id in ["00829", "00785"] {
                let mut matches = folders
                    .iter()
                    .map(|folder| PathBuf::from(folder).join(torrent_id))
                    .filter(|candidate| candidate.is_dir())
                    .collect::<Vec<_>>();
                if let Some(path) = matches.drain(..).next() {
                    debug!(path = %path.display(), torrent_id, "resolved game root from beamdog settings");
                    result = Some(path);
                    break;
                }
            }
        }
    }

    let Some(result) = result else {
        return Err(GameError::msg("Could not locate NWN; try --root"));
    };
    if !result.is_dir() {
        return Err(GameError::msg("Could not locate NWN; try --root"));
    }

    if !result.join("databuild.txt").is_file() {
        warn!(path = %result.display(), "NWN root does not contain databuild.txt");
    } else {
        let _ = fs::read_to_string(result.join("databuild.txt"))
            .ok()
            .and_then(|data| data.lines().next().map(str::trim).map(str::to_string));
    }

    info!(path = %result.display(), "resolved game root");
    Ok(result)
}

#[cfg(target_os = "linux")]
pub(crate) fn current_platform() -> Platform {
    Platform::Linux
}

#[cfg(target_os = "macos")]
pub(crate) fn current_platform() -> Platform {
    Platform::MacOs
}

#[cfg(target_os = "windows")]
pub(crate) fn current_platform() -> Platform {
    Platform::Windows
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
compile_error!("Unsupported os for game crate");

pub(crate) fn current_home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                let mut joined = PathBuf::from(drive);
                joined.push(path);
                Some(joined)
            })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn first_nonempty_path(
    override_dir: &str,
    primary_env: Option<&str>,
    secondary_env: Option<&str>,
    fallback: Option<PathBuf>,
) -> Option<PathBuf> {
    if !override_dir.is_empty() {
        return Some(PathBuf::from(override_dir));
    }
    if let Some(value) = primary_env.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    if let Some(value) = secondary_env.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    fallback
}

pub(crate) fn steamapps_dir<H>(platform: Platform, home_dir: &H) -> PathBuf
where
    H: Fn() -> Option<PathBuf>,
{
    match platform {
        Platform::MacOs => home_dir()
            .unwrap_or_default()
            .join("Library")
            .join("Application Support")
            .join("Steam")
            .join("steamapps")
            .join("common"),
        Platform::Linux => home_dir()
            .unwrap_or_default()
            .join(".local")
            .join("share")
            .join("Steam")
            .join("steamapps")
            .join("common"),
        Platform::Windows => PathBuf::from(r"c:\program files (x86)\steam\steamapps\common"),
    }
}

pub(crate) fn beamdog_settings_path<H>(platform: Platform, home_dir: &H) -> PathBuf
where
    H: Fn() -> Option<PathBuf>,
{
    match platform {
        Platform::MacOs => home_dir()
            .unwrap_or_default()
            .join("Library")
            .join("Application Support")
            .join("Beamdog Client")
            .join("settings.json"),
        Platform::Linux => home_dir()
            .unwrap_or_default()
            .join(".config")
            .join("Beamdog Client")
            .join("settings.json"),
        Platform::Windows => home_dir()
            .unwrap_or_default()
            .join("AppData")
            .join("Roaming")
            .join("Beamdog Client")
            .join("settings.json"),
    }
}

pub(crate) fn normalize_relative_path(input: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for segment in input
        .split(['\\', '/'])
        .filter(|segment| !segment.is_empty())
    {
        path.push(segment);
    }
    path
}

pub(crate) fn expand_tilde(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str == "~" {
        current_home_dir().unwrap_or_else(|| path.to_path_buf())
    } else if let Some(rest) = path_str.strip_prefix("~/") {
        current_home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest)
    } else {
        path.to_path_buf()
    }
}

fn language_aliases(language: &str) -> &'static [&'static str] {
    match language.to_ascii_lowercase().as_str() {
        "english" => &["en"],
        "en" => &["english"],
        "german" | "deutsch" => &["de"],
        "de" => &["german", "deutsch"],
        "spanish" => &["es"],
        "es" => &["spanish"],
        "french" => &["fr"],
        "fr" => &["french"],
        "italian" => &["it"],
        "it" => &["italian"],
        "polish" => &["pl"],
        "pl" => &["polish"],
        _ => &[],
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{expand_tilde, normalize_relative_path, resolve_language_root};
    use crate::{Platform, find_nwnrs_root_impl, find_user_root_impl};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-game-{prefix}-{nanos}"))
    }

    #[test]
    fn user_root_prefers_environment_over_platform_fallback() {
        let root = unique_test_dir("user-root");
        let env_dir = root.join("env");
        let home = root.join("home");
        let fallback = home.join(".local").join("share").join("Neverwinter Nights");
        if let Err(error) = fs::create_dir_all(&env_dir) {
            panic!("create env dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(&fallback) {
            panic!("create fallback dir: {error}");
        }

        let resolved = match find_user_root_impl(
            "",
            |key| match key {
                "nwnrs_HOME" => Some(env_dir.display().to_string()),
                _ => None,
            },
            || Some(home.clone()),
            Platform::Linux,
        ) {
            Ok(value) => value,
            Err(error) => panic!("resolve user root: {error}"),
        };
        assert_eq!(resolved, env_dir);
    }

    #[test]
    fn game_root_falls_back_to_beamdog_settings() {
        let root = unique_test_dir("game-root");
        let home = root.join("home");
        let beamdog_root = root.join("beamdog");
        let install = beamdog_root.join("00829");
        let settings = home
            .join(".config")
            .join("Beamdog Client")
            .join("settings.json");

        if let Err(error) = fs::create_dir_all(&install) {
            panic!("create install dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(settings.parent().unwrap_or(&home)) {
            panic!("create settings dir: {error}");
        }
        if let Err(error) = fs::write(
            &settings,
            format!(r#"{{"folders":["{}"]}}"#, beamdog_root.display()),
        ) {
            panic!("write settings: {error}");
        }
        if let Err(error) = fs::write(install.join("databuild.txt"), "build") {
            panic!("write databuild: {error}");
        }

        let resolved =
            match find_nwnrs_root_impl("", |_key| None, || Some(home.clone()), Platform::Linux) {
                Ok(value) => value,
                Err(error) => panic!("resolve game root: {error}"),
            };
        assert_eq!(resolved, install);
    }

    #[test]
    fn normalizes_relative_paths_and_expands_home() {
        assert_eq!(
            normalize_relative_path(r"foo\bar/baz"),
            PathBuf::from("foo/bar/baz")
        );
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(
                expand_tilde(&PathBuf::from("~/override")),
                PathBuf::from(home).join("override")
            );
        }
    }

    #[test]
    fn resolves_language_alias_to_short_folder_name() {
        let root = unique_test_dir("language-alias");
        let alias_root = root.join("lang").join("en");
        if let Err(error) = fs::create_dir_all(&alias_root) {
            panic!("create alias dir: {error}");
        }

        let resolved = match resolve_language_root(&root, "english") {
            Ok(value) => value,
            Err(error) => panic!("resolve english alias: {error}"),
        };
        assert_eq!(resolved, alias_root);
    }
}
