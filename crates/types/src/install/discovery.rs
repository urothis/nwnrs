use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tracing::{debug, info, instrument, warn};

use crate::install::{InstallError, InstallResult, Platform};

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
/// Resolution order is: explicit override, `NWN_HOME`, then the
/// platform-specific default location.
///
/// The function returns the first existing directory when possible. If no
/// candidate exists, it returns an error describing the accepted overrides.
///
/// # Errors
///
/// Returns [`InstallError`] if no candidate directory exists.
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
/// Resolution order is: explicit override, `NWN_ROOT`, Steam install
/// heuristics, Beamdog client settings heuristics, then legacy BioWare 1.69
/// install heuristics.
///
/// The returned path is required to exist as a directory. A missing
/// `databuild.txt` is treated as a warning rather than a hard failure so local
/// development layouts remain usable.
///
/// # Errors
///
/// Returns [`InstallError`] if no valid installation directory can be found.
#[instrument(level = "info", skip_all, err, fields(override_dir))]
pub fn find_nwnrs_root(override_dir: &str) -> InstallResult<PathBuf> {
    find_nwnrs_root_impl_with_registry(
        override_dir,
        |key| std::env::var(key).ok(),
        current_home_dir,
        current_platform(),
        read_windows_registry_string,
    )
}

/// Resolves an NWN language folder under `root/lang`, accepting both common
/// long-form names such as `english` and short on-disk codes such as `en`.
///
/// This function does not guess beyond the known alias table. Failure therefore
/// means the requested language root is genuinely absent under the provided
/// installation.
///
/// # Errors
///
/// Returns [`InstallError`] if no matching language directory exists under
/// `root/lang`.
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
    if let Some(preferred) =
        first_nonempty_path(override_dir, env_get("NWN_HOME").as_deref(), None, None)
    {
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
        "Could not locate NWN user directory; try --userdirectory or set NWN_HOME",
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
    skip(env_get, home_dir, registry_get),
    err,
    fields(override_dir, platform = ?platform)
)]
fn find_nwnrs_root_impl_with_registry<E, H, R>(
    override_dir: &str,
    env_get: E,
    home_dir: H,
    platform: Platform,
    registry_get: R,
) -> InstallResult<PathBuf>
where
    E: Fn(&str) -> Option<String>,
    H: Fn() -> Option<PathBuf>,
    R: Fn(&str, &str) -> Option<String>,
{
    debug!("resolving install root");
    if let Some(preferred) =
        first_nonempty_path(override_dir, env_get("NWN_ROOT").as_deref(), None, None)
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
    collect_steam_install_candidates(&mut candidates, platform, &home_dir, &registry_get);
    collect_beamdog_install_candidates(&mut candidates, platform, &home_dir)?;
    collect_legacy_install_candidates(&mut candidates, platform, &registry_get);

    let result = resolve_existing_dir(
        candidates,
        "Could not locate NWN; try --root",
        |candidate| is_valid_install_candidate(candidate),
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
    if path.join("databuild.txt").is_file() {
        let _ = fs::read_to_string(path.join("databuild.txt"))
            .ok()
            .and_then(|data| data.lines().next().map(str::trim).map(str::to_string));
    } else {
        warn!(path = %path.display(), "NWN root does not contain databuild.txt");
    }
}

fn collect_steam_install_candidates<H>(
    candidates: &mut Vec<CandidatePath>,
    platform: Platform,
    home_dir: &H,
    registry_get: &impl Fn(&str, &str) -> Option<String>,
) where
    H: Fn() -> Option<PathBuf>,
{
    collect_windows_steam_registry_candidates(candidates, platform, registry_get);
    for steam_root in steam_root_candidates(platform, home_dir) {
        collect_steam_root_candidates(candidates, &steam_root);
    }
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
        for install in beamdog_install_candidates(&root) {
            push_candidate(candidates, install, "beamdog");
        }
    }
    Ok(())
}

fn collect_legacy_install_candidates(
    candidates: &mut Vec<CandidatePath>,
    platform: Platform,
    registry_get: &impl Fn(&str, &str) -> Option<String>,
) {
    if platform != Platform::Windows {
        return;
    }

    for key in windows_legacy_registry_keys() {
        if let Some(location) = registry_get(key, "Location") {
            push_candidate(candidates, PathBuf::from(location), "legacy");
        }
    }

    for path in windows_legacy_default_paths() {
        push_candidate(candidates, path, "legacy");
    }
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

fn steam_root_candidates<H>(platform: Platform, home_dir: &H) -> Vec<PathBuf>
where
    H: Fn() -> Option<PathBuf>,
{
    match platform {
        Platform::MacOs => home_dir().map_or_else(Vec::new, |home| {
            vec![
                home.join("Library")
                    .join("Application Support")
                    .join("Steam"),
            ]
        }),
        Platform::Linux => home_dir().map_or_else(Vec::new, |home| {
            vec![
                home.join(".local").join("share").join("Steam"),
                home.join(".steam").join("steam"),
                home.join(".var")
                    .join("app")
                    .join("com.valvesoftware.Steam")
                    .join(".local")
                    .join("share")
                    .join("Steam"),
            ]
        }),
        Platform::Windows => {
            let mut roots = windows_system_steam_roots();
            if let Some(home) = home_dir() {
                push_unique_path(&mut roots, home.join("AppData").join("Local").join("Steam"));
            }
            roots
        }
    }
}

fn windows_system_steam_roots() -> Vec<PathBuf> {
    #[cfg(all(target_os = "windows", test))]
    {
        // Unit tests inject their own home directory and should not consult the
        // host machine's global Steam install.
        Vec::new()
    }

    #[cfg(not(all(target_os = "windows", test)))]
    {
        vec![PathBuf::from(r"c:\program files (x86)\steam")]
    }
}

fn collect_steam_root_candidates(candidates: &mut Vec<CandidatePath>, steam_root: &Path) {
    let library_roots = steam_library_roots(steam_root);
    for library_root in library_roots {
        let steamapps = library_root.join("steamapps");
        if let Some(install_dir) = steam_app_install_dir(&steamapps) {
            push_candidate(
                candidates,
                steamapps.join("common").join(install_dir),
                "steam",
            );
        }

        push_candidate(
            candidates,
            steamapps.join("common").join("Neverwinter Nights"),
            "steam",
        );
    }
}

fn steam_library_roots(steam_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    push_unique_path(&mut roots, steam_root.to_path_buf());

    for path in [
        steam_root.join("steamapps").join("libraryfolders.vdf"),
        steam_root.join("config").join("libraryfolders.vdf"),
    ] {
        let Ok(data) = fs::read_to_string(&path) else {
            continue;
        };

        for root in parse_steam_libraryfolders(&data) {
            push_unique_path(&mut roots, root);
        }
    }

    roots
}

fn collect_windows_steam_registry_candidates(
    candidates: &mut Vec<CandidatePath>,
    platform: Platform,
    registry_get: &impl Fn(&str, &str) -> Option<String>,
) {
    if platform != Platform::Windows {
        return;
    }

    for key in windows_steam_registry_keys() {
        if let Some(location) = registry_get(key, "InstallLocation") {
            push_candidate(candidates, PathBuf::from(location), "steam");
        }
    }
}

fn steam_app_install_dir(steamapps: &Path) -> Option<String> {
    let manifest = steamapps.join("appmanifest_704450.acf");
    let data = fs::read_to_string(manifest).ok()?;
    parse_steam_appmanifest_installdir(&data)
}

fn parse_steam_libraryfolders(data: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for line in data.lines() {
        let Some((key, value)) = parse_vdf_key_value(line) else {
            continue;
        };
        if key == "path" {
            push_unique_path(&mut roots, steam_vdf_path_value(&value));
        }
    }
    roots
}

fn parse_steam_appmanifest_installdir(data: &str) -> Option<String> {
    data.lines().find_map(|line| {
        let (key, value) = parse_vdf_key_value(line)?;
        (key == "installdir").then_some(value)
    })
}

fn parse_vdf_key_value(line: &str) -> Option<(String, String)> {
    let mut parts = line.split('"').skip(1);
    let key = parts.next()?.trim().to_string();
    let value = parts.nth(1)?.trim().to_string();
    Some((key, value))
}

fn steam_vdf_path_value(value: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    let value = value.replace("\\\\", "\\");
    #[cfg(not(target_os = "windows"))]
    let value = value.to_string();
    PathBuf::from(value)
}

fn beamdog_install_candidates(root: &Path) -> Vec<PathBuf> {
    let mut installs = Vec::new();
    push_unique_path(&mut installs, root.to_path_buf());

    let Ok(entries) = fs::read_dir(root) else {
        return installs;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if name.chars().all(|ch| ch.is_ascii_digit()) {
            push_unique_path(&mut installs, path);
        }
    }

    installs
}

fn is_valid_install_candidate(candidate: &CandidatePath) -> bool {
    match candidate.source {
        "steam" => looks_like_steam_install_root(&candidate.path),
        "beamdog" | "legacy" => looks_like_nwn_install_root(&candidate.path),
        _ => candidate.path.is_dir(),
    }
}

fn looks_like_steam_install_root(path: &Path) -> bool {
    looks_like_nwn_install_root(path) && path.join("steam_appid.txt").is_file()
}

fn looks_like_nwn_install_root(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    let has_data = path.join("data").is_dir();
    let has_bin = path.join("bin").is_dir();
    let has_lang = path.join("lang").is_dir();
    let has_databuild = path.join("databuild.txt").is_file();
    let has_nwn_ini = path.join("nwn.ini").is_file();
    let has_dialog = path.join("dialog.tlk").is_file()
        || path
            .join("lang")
            .join("en")
            .join("data")
            .join("dialog.tlk")
            .is_file();

    has_data && (has_databuild || has_dialog || has_lang || has_bin || has_nwn_ini)
}

fn windows_steam_registry_keys() -> &'static [&'static str] {
    &[
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Steam App 704450",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Steam App 704450",
    ]
}

fn windows_legacy_registry_keys() -> &'static [&'static str] {
    &[
        r"SOFTWARE\BioWare\NWN\Neverwinter",
        r"SOFTWARE\WOW6432Node\BioWare\NWN\Neverwinter",
    ]
}

fn windows_legacy_default_paths() -> [PathBuf; 2] {
    [
        PathBuf::from(r"c:\program files\neverwinter nights"),
        PathBuf::from(r"c:\program files (x86)\neverwinter nights"),
    ]
}

#[cfg(target_os = "windows")]
fn read_windows_registry_string(key_path: &str, value_name: &str) -> Option<String> {
    use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm.open_subkey(key_path).ok()?;
    let value: String = key.get_value(value_name).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(not(target_os = "windows"))]
fn read_windows_registry_string(_key_path: &str, _value_name: &str) -> Option<String> {
    None
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if paths.iter().any(|candidate| candidate == &path) {
        return;
    }
    paths.push(path);
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::json;

    use super::{
        beamdog_install_candidates, expand_tilde, find_nwnrs_root_impl_with_registry,
        normalize_relative_path, resolve_language_root, windows_legacy_default_paths,
        windows_legacy_registry_keys, windows_steam_registry_keys,
    };
    use crate::install::{Platform, find_user_root_impl};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-types-{prefix}-{nanos}"))
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
                "NWN_HOME" => Some(env_dir.display().to_string()),
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

        if let Err(error) = fs::create_dir_all(install.join("data")) {
            panic!("create install dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(settings.parent().unwrap_or(&home)) {
            panic!("create settings dir: {error}");
        }
        if let Err(error) = fs::write(&settings, json!({ "folders": [beamdog_root] }).to_string()) {
            panic!("write settings: {error}");
        }
        if let Err(error) = fs::write(install.join("databuild.txt"), "build") {
            panic!("write databuild: {error}");
        }

        let resolved = match find_nwnrs_root_impl_with_registry(
            "",
            |_key| None,
            || Some(home.clone()),
            Platform::Linux,
            |_key, _value| None,
        ) {
            Ok(value) => value,
            Err(error) => panic!("resolve game root: {error}"),
        };
        assert_eq!(resolved, install);
    }

    #[test]
    fn beamdog_install_candidates_include_unknown_numeric_children() {
        let root = unique_test_dir("beamdog-candidates");
        let library = root.join("beamdog");
        let install = library.join("00832");
        let other = library.join("notes");

        if let Err(error) = fs::create_dir_all(&install) {
            panic!("create beamdog numeric dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(&other) {
            panic!("create beamdog non-numeric dir: {error}");
        }

        let candidates = beamdog_install_candidates(&library);
        assert!(candidates.contains(&library));
        assert!(candidates.contains(&install));
        assert!(!candidates.contains(&other));
    }

    #[test]
    fn game_root_detects_steam_install_from_custom_libraryfolders() {
        let root = unique_test_dir("steam-libraryfolders");
        let home = root.join("home");
        let steam_root = home.join(".local").join("share").join("Steam");
        let library_root = root.join("steam-library");
        let install = library_root.join("steamapps").join("common").join("NWN EE");
        let libraryfolders = steam_root.join("steamapps").join("libraryfolders.vdf");
        let manifest = library_root
            .join("steamapps")
            .join("appmanifest_704450.acf");

        if let Err(error) = fs::create_dir_all(install.join("data")) {
            panic!("create steam data dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(install.join("lang")) {
            panic!("create steam lang dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(libraryfolders.parent().unwrap_or(&steam_root)) {
            panic!("create steamapps dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(manifest.parent().unwrap_or(&library_root)) {
            panic!("create manifest dir: {error}");
        }
        if let Err(error) = fs::write(
            &libraryfolders,
            format!(
                concat!(
                    "\"libraryfolders\"\n",
                    "{{\n",
                    "\t\"0\"\n",
                    "\t{{\n",
                    "\t\t\"path\"\t\t\"{}\"\n",
                    "\t}}\n",
                    "\t\"1\"\n",
                    "\t{{\n",
                    "\t\t\"path\"\t\t\"{}\"\n",
                    "\t\t\"apps\"\n",
                    "\t\t{{\n",
                    "\t\t\t\"704450\"\t\t\"1\"\n",
                    "\t\t}}\n",
                    "\t}}\n",
                    "}}\n"
                ),
                steam_root.display(),
                library_root.display(),
            ),
        ) {
            panic!("write libraryfolders.vdf: {error}");
        }
        if let Err(error) = fs::write(
            &manifest,
            "\"AppState\"\n{\n\t\"appid\"\t\t\"704450\"\n\t\"installdir\"\t\t\"NWN EE\"\n}\n",
        ) {
            panic!("write appmanifest: {error}");
        }
        if let Err(error) = fs::write(install.join("steam_appid.txt"), "704450\n") {
            panic!("write steam_appid.txt: {error}");
        }
        if let Err(error) = fs::write(install.join("databuild.txt"), "build") {
            panic!("write databuild.txt: {error}");
        }

        let resolved = match find_nwnrs_root_impl_with_registry(
            "",
            |_key| None,
            || Some(home.clone()),
            Platform::Linux,
            |_key, _value| None,
        ) {
            Ok(value) => value,
            Err(error) => panic!("resolve steam install root: {error}"),
        };
        assert_eq!(resolved, install);
    }

    #[test]
    fn game_root_uses_manifest_installdir_for_default_steam_library() {
        let root = unique_test_dir("steam-default-manifest");
        let home = root.join("home");
        let steam_root = home.join(".local").join("share").join("Steam");
        let steamapps = steam_root.join("steamapps");
        let install = steamapps.join("common").join("NWN Alt");
        let manifest = steamapps.join("appmanifest_704450.acf");

        if let Err(error) = fs::create_dir_all(install.join("data")) {
            panic!("create default steam data dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(install.join("lang")) {
            panic!("create default steam lang dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(&steamapps) {
            panic!("create default steamapps dir: {error}");
        }
        if let Err(error) = fs::write(
            &manifest,
            "\"AppState\"\n{\n\t\"appid\"\t\t\"704450\"\n\t\"installdir\"\t\t\"NWN Alt\"\n}\n",
        ) {
            panic!("write default appmanifest: {error}");
        }
        if let Err(error) = fs::write(install.join("steam_appid.txt"), "704450\n") {
            panic!("write default steam_appid.txt: {error}");
        }
        if let Err(error) = fs::write(install.join("databuild.txt"), "build") {
            panic!("write default databuild.txt: {error}");
        }

        let resolved = match find_nwnrs_root_impl_with_registry(
            "",
            |_key| None,
            || Some(home.clone()),
            Platform::Linux,
            |_key, _value| None,
        ) {
            Ok(value) => value,
            Err(error) => panic!("resolve default steam install root: {error}"),
        };
        assert_eq!(resolved, install);
    }

    #[test]
    fn windows_game_root_prefers_steam_registry_install_location() {
        let root = unique_test_dir("windows-steam-registry");
        let home = root.join("home");
        let install = root.join("steam-install");

        if let Err(error) = fs::create_dir_all(install.join("data")) {
            panic!("create steam registry data dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(install.join("lang")) {
            panic!("create steam registry lang dir: {error}");
        }
        if let Err(error) = fs::write(install.join("steam_appid.txt"), "704450\n") {
            panic!("write steam registry steam_appid.txt: {error}");
        }
        if let Err(error) = fs::write(install.join("databuild.txt"), "build") {
            panic!("write steam registry databuild.txt: {error}");
        }

        let resolved = match find_nwnrs_root_impl_with_registry(
            "",
            |_key| None,
            || Some(home.clone()),
            Platform::Windows,
            |key, value| {
                (key == windows_steam_registry_keys()[0] && value == "InstallLocation")
                    .then(|| install.display().to_string())
            },
        ) {
            Ok(value) => value,
            Err(error) => panic!("resolve steam registry install root: {error}"),
        };
        assert_eq!(resolved, install);
    }

    #[test]
    fn windows_game_root_prefers_steam_over_legacy_registry_location() {
        let root = unique_test_dir("windows-steam-over-legacy");
        let home = root.join("home");
        let steam_install = root.join("steam-install");
        let legacy_install = root.join("legacy-install");

        if let Err(error) = fs::create_dir_all(steam_install.join("data")) {
            panic!("create steam-over-legacy steam data dir: {error}");
        }
        if let Err(error) = fs::create_dir_all(steam_install.join("lang")) {
            panic!("create steam-over-legacy steam lang dir: {error}");
        }
        if let Err(error) = fs::write(steam_install.join("steam_appid.txt"), "704450\n") {
            panic!("write steam-over-legacy steam_appid.txt: {error}");
        }
        if let Err(error) = fs::write(steam_install.join("databuild.txt"), "build") {
            panic!("write steam-over-legacy databuild.txt: {error}");
        }

        if let Err(error) = fs::create_dir_all(legacy_install.join("data")) {
            panic!("create steam-over-legacy legacy data dir: {error}");
        }
        if let Err(error) = fs::write(legacy_install.join("nwn.ini"), "[Alias]\n") {
            panic!("write steam-over-legacy legacy nwn.ini: {error}");
        }

        let resolved = match find_nwnrs_root_impl_with_registry(
            "",
            |_key| None,
            || Some(home.clone()),
            Platform::Windows,
            |key, value| {
                if key == windows_steam_registry_keys()[0] && value == "InstallLocation" {
                    return Some(steam_install.display().to_string());
                }
                (key == windows_legacy_registry_keys()[0] && value == "Location")
                    .then(|| legacy_install.display().to_string())
            },
        ) {
            Ok(value) => value,
            Err(error) => panic!("resolve steam-over-legacy install root: {error}"),
        };
        assert_eq!(resolved, steam_install);
    }

    #[test]
    fn windows_game_root_falls_back_to_legacy_registry_location_when_no_steam_sources_exist() {
        let root = unique_test_dir("windows-legacy-registry");
        let home = root.join("home");
        let install = root.join("legacy-install");

        if let Err(error) = fs::create_dir_all(install.join("data")) {
            panic!("create legacy registry data dir: {error}");
        }
        if let Err(error) = fs::write(install.join("nwn.ini"), "[Alias]\n") {
            panic!("write legacy registry nwn.ini: {error}");
        }

        let resolved = match find_nwnrs_root_impl_with_registry(
            "",
            |_key| None,
            || Some(home.clone()),
            Platform::Windows,
            |key, value| {
                (key == windows_legacy_registry_keys()[0] && value == "Location")
                    .then(|| install.display().to_string())
            },
        ) {
            Ok(value) => value,
            Err(error) => panic!("resolve legacy registry install root: {error}"),
        };
        assert_eq!(resolved, install);
    }

    #[test]
    fn windows_legacy_default_paths_include_both_program_files_roots() {
        let defaults = windows_legacy_default_paths();
        assert_eq!(
            defaults[0],
            PathBuf::from(r"c:\program files\neverwinter nights")
        );
        assert_eq!(
            defaults[1],
            PathBuf::from(r"c:\program files (x86)\neverwinter nights")
        );
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
