use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tracing::{debug, info, instrument, warn};

use crate::{InstallError, InstallResult, Platform};

#[derive(Debug, Deserialize)]
struct BeamdogSettings {
    folders: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct CandidatePath {
    path:   PathBuf,
    source: &'static str,
}

/// Locates the NWN user directory.
///
/// Resolution order is: explicit override, `nwnrs_HOME`,
/// `nwnrs_USER_DIRECTORY`, then the platform-specific default location.
///
/// The function returns the first existing directory when possible. If no
/// candidate exists, it returns an error describing the accepted overrides.
#[instrument(level = "info", skip_all, err, fields(override_dir))]
pub fn find_user_root(override_dir: &str) -> InstallResult<PathBuf> {
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
///
/// The returned path is required to exist as a directory. A missing
/// `databuild.txt` is treated as a warning rather than a hard failure so local
/// development layouts remain usable.
#[instrument(level = "info", skip_all, err, fields(override_dir))]
pub fn find_nwnrs_root(override_dir: &str) -> InstallResult<PathBuf> {
    find_nwnrs_root_impl(
        override_dir,
        |key| std::env::var(key).ok(),
        current_home_dir,
        current_platform(),
    )
}

/// Resolves an NWN language folder under `root/lang`, accepting both common
/// long-form names such as `english` and short on-disk codes such as `en`.
///
/// This function does not guess beyond the known alias table. Failure therefore
/// means the requested language root is genuinely absent under the provided
/// installation.
#[instrument(level = "info", skip_all, err, fields(root = %root.as_ref().display(), language))]
pub fn resolve_language_root(root: impl AsRef<Path>, language: &str) -> InstallResult<PathBuf> {
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

    Err(InstallError::msg(format!(
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
) -> InstallResult<PathBuf>
where
    E: Fn(&str) -> Option<String>,
    H: Fn() -> Option<PathBuf>,
{
    debug!("resolving user root");
    if let Some(preferred) = first_nonempty_path(
        override_dir,
        env_get("nwnrs_HOME").as_deref(),
        env_get("nwnrs_USER_DIRECTORY").as_deref(),
        None,
    ) {
        return preferred
            .is_dir()
            .then_some(preferred.clone())
            .ok_or_else(|| {
                InstallError::msg(format!(
                    "requested user directory does not exist: {}",
                    preferred.display()
                ))
            })
            .inspect(|path| info!(path = %path.display(), source = "explicit-or-env", "resolved directory"));
    }

    let mut candidates = Vec::new();
    for path in user_root_candidates(platform, &home_dir) {
        push_candidate(&mut candidates, path, "platform-default");
    }

    resolve_existing_dir(
        candidates,
        "Could not locate NWN user directory; try --userdirectory or set nwnrs_HOME \
         (nwnrs_USER_DIRECTORY also works, but is considered alternate)",
        |candidate| candidate.path.is_dir(),
    )
}

fn user_root_candidates<H>(platform: Platform, home_dir: &H) -> Vec<PathBuf>
where
    H: Fn() -> Option<PathBuf>,
{
    let Some(home) = home_dir() else {
        return Vec::new();
    };

    match platform {
        Platform::MacOs => vec![
            home.join("Documents").join("Neverwinter Nights"),
            home.join("Library")
                .join("Application Support")
                .join("Neverwinter Nights"),
        ],
        Platform::Linux => vec![home.join(".local").join("share").join("Neverwinter Nights")],
        Platform::Windows => vec![home.join("Documents").join("Neverwinter Nights")],
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
) -> InstallResult<PathBuf>
where
    E: Fn(&str) -> Option<String>,
    H: Fn() -> Option<PathBuf>,
{
    debug!("resolving install root");
    if let Some(preferred) =
        first_nonempty_path(override_dir, env_get("nwnrs_ROOT").as_deref(), None, None)
    {
        if !preferred.is_dir() {
            return Err(InstallError::msg(format!(
                "requested NWN root does not exist: {}",
                preferred.display()
            )));
        }
        validate_install_root(&preferred);
        info!(path = %preferred.display(), source = "explicit-or-env", "resolved directory");
        return Ok(preferred);
    }

    let mut candidates = Vec::new();
    collect_steam_install_candidates(&mut candidates, platform, &home_dir);
    collect_beamdog_install_candidates(&mut candidates, platform, &home_dir)?;

    let result = resolve_existing_dir(
        candidates,
        "Could not locate NWN; try --root",
        |candidate| match candidate.source {
            "steam" => {
                candidate.path.is_dir()
                    && candidate.path.join("data").is_dir()
                    && candidate.path.join("steam_appid.txt").is_file()
            }
            _ => candidate.path.is_dir(),
        },
    )?;
    validate_install_root(&result);
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
compile_error!("Unsupported os for install crate");

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

fn push_candidate(candidates: &mut Vec<CandidatePath>, path: PathBuf, source: &'static str) {
    if candidates.iter().any(|candidate| candidate.path == path) {
        return;
    }
    candidates.push(CandidatePath {
        path,
        source,
    });
}

fn resolve_existing_dir(
    candidates: Vec<CandidatePath>,
    missing_message: &'static str,
    is_valid: impl Fn(&CandidatePath) -> bool,
) -> InstallResult<PathBuf> {
    let Some(candidate) = candidates.into_iter().find(is_valid) else {
        return Err(InstallError::msg(missing_message));
    };

    info!(path = %candidate.path.display(), source = candidate.source, "resolved directory");
    Ok(candidate.path)
}

fn validate_install_root(path: &Path) {
    if !path.join("databuild.txt").is_file() {
        warn!(path = %path.display(), "NWN root does not contain databuild.txt");
    } else {
        let _ = fs::read_to_string(path.join("databuild.txt"))
            .ok()
            .and_then(|data| data.lines().next().map(str::trim).map(str::to_string));
    }
}

fn collect_steam_install_candidates<H>(
    candidates: &mut Vec<CandidatePath>,
    platform: Platform,
    home_dir: &H,
) where
    H: Fn() -> Option<PathBuf>,
{
    push_candidate(
        candidates,
        steamapps_dir(platform, home_dir).join("Neverwinter Nights"),
        "steam",
    );
}

fn collect_beamdog_install_candidates<H>(
    candidates: &mut Vec<CandidatePath>,
    platform: Platform,
    home_dir: &H,
) -> InstallResult<()>
where
    H: Fn() -> Option<PathBuf>,
{
    for root in beamdog_install_roots(platform, home_dir)? {
        for torrent_id in ["00829", "00785"] {
            push_candidate(candidates, root.join(torrent_id), "beamdog");
        }
    }
    Ok(())
}

fn beamdog_install_roots<H>(platform: Platform, home_dir: &H) -> InstallResult<Vec<PathBuf>>
where
    H: Fn() -> Option<PathBuf>,
{
    let settings_file = beamdog_settings_path(platform, home_dir);
    if !settings_file.is_file() {
        return Ok(Vec::new());
    }

    let data = fs::read_to_string(&settings_file)?;
    let settings: BeamdogSettings = serde_json::from_str(&data)?;
    let Some(folders) = settings.folders else {
        return Err(InstallError::msg("Beamdog settings missing folders array"));
    };

    Ok(folders.into_iter().map(PathBuf::from).collect())
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
        std::env::temp_dir().join(format!("nwnrs-install-{prefix}-{nanos}"))
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
    fn macos_user_root_falls_back_to_application_support() {
        let root = unique_test_dir("mac-user-root");
        let home = root.join("home");
        let fallback = home
            .join("Library")
            .join("Application Support")
            .join("Neverwinter Nights");
        if let Err(error) = fs::create_dir_all(&fallback) {
            panic!("create mac fallback dir: {error}");
        }

        let resolved =
            match find_user_root_impl("", |_key| None, || Some(home.clone()), Platform::MacOs) {
                Ok(value) => value,
                Err(error) => panic!("resolve mac user root: {error}"),
            };
        assert_eq!(resolved, fallback);
    }

    #[test]
    fn macos_user_root_prefers_documents_per_readme() {
        let root = unique_test_dir("mac-user-root-documents");
        let home = root.join("home");
        let documents = home.join("Documents").join("Neverwinter Nights");
        let application_support = home
            .join("Library")
            .join("Application Support")
            .join("Neverwinter Nights");
        if let Err(error) = fs::create_dir_all(&documents) {
            panic!("create mac documents dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(&application_support) {
            panic!("create mac application support dir: {error}");
        }

        let resolved =
            match find_user_root_impl("", |_key| None, || Some(home.clone()), Platform::MacOs) {
                Ok(value) => value,
                Err(error) => panic!("resolve mac user root: {error}"),
            };
        assert_eq!(resolved, documents);
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
