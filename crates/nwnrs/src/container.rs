use std::{
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use tracing::{info, warn};

const PERSISTENT_HOME: &str = "/nwn/home";
const RUNTIME_HOME: &str = "/nwn/run";
const RUNTIME_LIBRARY: &str = "/nwn/runtime/libnwnrs_runtime_sys.so";
const TARGETS_DIRECTORY: &str = "/nwn/runtime/targets";

const PERSISTENT_DIRECTORIES: [&str; 10] = [
    "database",
    "hak",
    "modules",
    "nwsync",
    "override",
    "portraits",
    "saves",
    "servervault",
    "tlk",
    "development",
];

const OPTIONAL_LINKED_FILES: [&str; 3] = ["dialog.tlk", "dialogf.tlk", "settings.tml"];

pub(crate) struct ContainerLaunch {
    pub(crate) state:             ContainerState,
    pub(crate) server:            PathBuf,
    pub(crate) runtime:           PathBuf,
    pub(crate) targets:           PathBuf,
    pub(crate) working_directory: PathBuf,
    pub(crate) server_args:       Vec<String>,
    pub(crate) tail_logs:         bool,
}

pub(crate) struct ContainerState {
    persistent_home: PathBuf,
    runtime_home:    PathBuf,
    finalized:       bool,
}

impl ContainerLaunch {
    pub(crate) fn prepare(forwarded_args: &[String]) -> Result<Self, String> {
        let persistent_home = PathBuf::from(PERSISTENT_HOME);
        let runtime_home = PathBuf::from(RUNTIME_HOME);
        fs::create_dir_all(&persistent_home).map_err(|error| {
            format!(
                "failed to create persistent server directory {}: {error}",
                persistent_home.display()
            )
        })?;
        fs::create_dir_all(&runtime_home).map_err(|error| {
            format!(
                "failed to create runtime server directory {}: {error}",
                runtime_home.display()
            )
        })?;

        let state = ContainerState {
            persistent_home,
            runtime_home,
            finalized: false,
        };
        state.prepare_filesystem()?;
        state.import_configuration()?;

        let server = bundled_server_path()?;
        let working_directory = server
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| "bundled server path has no parent directory".to_string())?;
        Ok(Self {
            state,
            server,
            runtime: PathBuf::from(RUNTIME_LIBRARY),
            targets: PathBuf::from(TARGETS_DIRECTORY),
            working_directory,
            server_args: build_server_args(forwarded_args)?,
            tail_logs: env_value("NWN_TAIL_LOGS").is_none_or(|value| value == "y"),
        })
    }
}

impl ContainerState {
    fn prepare_filesystem(&self) -> Result<(), String> {
        info!(target: "nwnrs::container", "linking persistent server data");
        for name in PERSISTENT_DIRECTORIES {
            let persistent = self.persistent_home.join(name);
            fs::create_dir_all(&persistent).map_err(|error| {
                format!(
                    "failed to create persistent directory {}: {error}",
                    persistent.display()
                )
            })?;
            ensure_symlink(&persistent, &self.runtime_home.join(name))?;
        }

        for name in OPTIONAL_LINKED_FILES {
            let persistent = self.persistent_home.join(name);
            let runtime = self.runtime_home.join(name);
            if persistent.exists() && fs::symlink_metadata(&runtime).is_err() {
                ensure_symlink(&persistent, &runtime)?;
            }
        }
        Ok(())
    }

    fn import_configuration(&self) -> Result<(), String> {
        info!(target: "nwnrs::container", "importing configuration");
        let persistent_ini = self.persistent_home.join("nwn.ini");
        if persistent_ini.is_file() {
            info!(target: "nwnrs::container", file = "nwn.ini", "importing configuration file");
            let source = fs::read_to_string(&persistent_ini)
                .map_err(|error| format!("failed to read {}: {error}", persistent_ini.display()))?;
            let filtered = remove_alias_section(&source);
            let destination = self.runtime_home.join("nwn.ini");
            fs::write(&destination, filtered)
                .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;
        }

        for name in ["nwnplayer.ini", "cryptographic_secret"] {
            let source = self.persistent_home.join(name);
            if source.is_file() {
                info!(target: "nwnrs::container", file = name, "importing configuration file");
                copy_with_permissions(&source, &self.runtime_home.join(name))?;
            }
        }
        Ok(())
    }

    pub(crate) fn backup_configuration(&self) {
        for name in ["cryptographic_secret", "settings.tml"] {
            let source = self.runtime_home.join(name);
            let Ok(metadata) = fs::symlink_metadata(&source) else {
                continue;
            };
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            info!(target: "nwnrs::container", file = name, "backing up configuration file");
            if let Err(error) = atomic_copy(&source, &self.persistent_home.join(name)) {
                warn!(target: "nwnrs::container", file = name, %error, "failed to back up configuration file");
            }
        }
    }

    pub(crate) fn finalize(&mut self) {
        if self.finalized {
            return;
        }
        self.backup_configuration();
        self.preserve_crash_logs();
        self.finalized = true;
    }

    fn preserve_crash_logs(&self) {
        let Ok(entries) = fs::read_dir(&self.runtime_home) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };
            if !name_str.starts_with("nwserver-crash") || !name_str.ends_with(".log") {
                continue;
            }
            let source = entry.path();
            if !source.is_file() {
                continue;
            }
            info!(target: "nwnrs::container", file = name_str, "preserving NWServer crash log");
            if let Err(error) = copy_with_permissions(&source, &self.persistent_home.join(&name)) {
                warn!(target: "nwnrs::container", file = name_str, %error, "failed to preserve NWServer crash log");
            }
        }
    }
}

impl Drop for ContainerState {
    fn drop(&mut self) {
        self.finalize();
    }
}

fn bundled_server_path() -> Result<PathBuf, String> {
    let directory = match env::consts::ARCH {
        "aarch64" => "linux-arm64",
        "x86_64" => "linux-amd64",
        architecture => {
            return Err(format!(
                "the bundled container server does not support architecture {architecture}"
            ));
        }
    };
    Ok(Path::new("/nwn/data/bin").join(directory).join("nwserver"))
}

fn build_server_args(forwarded_args: &[String]) -> Result<Vec<String>, String> {
    let mut args: Vec<String> = env_value("NWN_EXTRA_ARGS")
        .map(|value| value.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    for (option, variable, default) in [
        ("-port", "NWN_PORT", "5121"),
        ("-servername", "NWN_SERVERNAME", "nwnrs server"),
        ("-module", "NWN_MODULE", "nwnrs"),
        ("-publicserver", "NWN_PUBLICSERVER", "0"),
        ("-maxclients", "NWN_MAXCLIENTS", "96"),
        ("-minlevel", "NWN_MINLEVEL", "1"),
        ("-maxlevel", "NWN_MAXLEVEL", "40"),
        ("-pauseandplay", "NWN_PAUSEANDPLAY", "1"),
        ("-pvp", "NWN_PVP", "2"),
        ("-servervault", "NWN_SERVERVAULT", "1"),
        ("-elc", "NWN_ELC", "1"),
        ("-ilr", "NWN_ILR", "1"),
        ("-gametype", "NWN_GAMETYPE", "0"),
        ("-oneparty", "NWN_ONEPARTY", "0"),
        ("-difficulty", "NWN_DIFFICULTY", "3"),
        ("-autosaveinterval", "NWN_AUTOSAVEINTERVAL", "0"),
        ("-reloadwhenempty", "NWN_RELOADWHENEMPTY", "0"),
    ] {
        args.push(option.to_string());
        args.push(env_value(variable).unwrap_or_else(|| default.to_string()));
    }
    args.extend([
        "-userdirectory".to_string(),
        RUNTIME_HOME.to_string(),
        "-interactive".to_string(),
    ]);

    for (option, variable) in [
        ("-playerpassword", "NWN_PLAYERPASSWORD_FILE"),
        ("-dmpassword", "NWN_DMPASSWORD_FILE"),
        ("-adminpassword", "NWN_ADMINPASSWORD_FILE"),
    ] {
        if let Some(path) = env_value(variable) {
            let secret = fs::read_to_string(&path)
                .map_err(|error| format!("{variable} must name a readable UTF-8 file: {error}"))?;
            args.extend([
                option.to_string(),
                secret.trim_end_matches(['\r', '\n']).to_string(),
            ]);
        }
    }
    for (option, variable) in [
        ("-nwsyncurl", "NWN_NWSYNCURL"),
        ("-nwsynchash", "NWN_NWSYNCHASH"),
    ] {
        if let Some(value) = env_value(variable) {
            args.extend([option.to_string(), value]);
        }
    }
    args.extend_from_slice(forwarded_args);
    Ok(args)
}

fn env_value(name: &str) -> Option<String> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string_lossy().into_owned())
}

fn ensure_symlink(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::symlink_metadata(destination) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "failed to inspect runtime path {}: {error}",
                destination.display()
            ));
        }
    }
    std::os::unix::fs::symlink(source, destination).map_err(|error| {
        format!(
            "failed to link {} to {}: {error}",
            destination.display(),
            source.display()
        )
    })
}

fn remove_alias_section(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut in_alias = false;
    for line in source.split_inclusive('\n') {
        let logical = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or_else(|| line.strip_suffix('\n').unwrap_or(line));
        if in_alias {
            if logical.starts_with('[') {
                in_alias = false;
            } else {
                continue;
            }
        }
        if logical.starts_with("[Alias]") {
            in_alias = true;
        }
        output.push_str(line);
    }
    output
}

fn copy_with_permissions(source: &Path, destination: &Path) -> Result<(), String> {
    fs::copy(source, destination).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    let permissions = fs::metadata(source)
        .map_err(|error| format!("failed to inspect {}: {error}", source.display()))?
        .permissions();
    fs::set_permissions(destination, permissions).map_err(|error| {
        format!(
            "failed to preserve permissions on {}: {error}",
            destination.display()
        )
    })
}

fn atomic_copy(source: &Path, destination: &Path) -> Result<(), String> {
    let name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("invalid destination filename: {}", destination.display()))?;
    let temporary = destination.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    copy_with_permissions(source, &temporary)?;
    fs::rename(&temporary, destination).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!(
            "failed to replace {} with {}: {error}",
            destination.display(),
            temporary.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::remove_alias_section;

    #[test]
    fn alias_section_filter_matches_the_previous_awk_behavior() {
        let source = "[Display Options]\nWidth=1920\n[Alias]\nHD0=/game\nMODULES=/mods\n[Server \
                      Options]\nMax Players=6\n";
        assert_eq!(
            remove_alias_section(source),
            "[Display Options]\nWidth=1920\n[Alias]\n[Server Options]\nMax Players=6\n"
        );
    }

    #[test]
    fn alias_section_filter_preserves_crlf_and_missing_final_newline() {
        let source = "[Alias]\r\nHD0=/game\r\n[Server Options]\r\nValue=1";
        assert_eq!(
            remove_alias_section(source),
            "[Alias]\r\n[Server Options]\r\nValue=1"
        );
    }
}
