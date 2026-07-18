#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod bridge;

use std::{
    env,
    error::Error,
    fmt,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

pub use bridge::{
    BridgeError, BridgeResult, BridgeValue, EventContext, ScriptBridge, ScriptLog, ScriptLogLevel,
    ServerState, Vector, event_name,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// The supported target-pack schema version.
pub const TARGET_PACK_SCHEMA_VERSION: u32 = 2;
/// The runtime API implemented by this version of the crate.
pub const RUNTIME_API_VERSION: u32 = 2;
/// Enables initialization when set to `1` in an injected process.
pub const ENV_ENABLED: &str = "NWNRS_ENABLED";
/// Makes initialization failure fatal when set to `1`.
pub const ENV_REQUIRED: &str = "NWNRS_REQUIRED";
/// Indicates that a supervising launcher owns final diagnostic rendering.
pub const ENV_SUPERVISED: &str = "NWNRS_SUPERVISED";
/// Names one exact target-pack file selected by the launcher.
pub const ENV_TARGET_PACK: &str = "NWNRS_TARGET_PACK";
/// Names the root directory used for hash-based target-pack lookup.
pub const ENV_TARGET_DIR: &str = "NWNRS_TARGET_DIR";

/// An error produced while identifying or configuring the runtime.
///
/// ```
/// let error: Option<nwnrs_runtime::RuntimeError> = None;
/// assert!(error.is_none());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    message: String,
}

impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RuntimeError {}

/// A result returned by runtime identification and configuration operations.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// An operating system supported by the native runtime.
///
/// ```
/// assert_eq!(nwnrs_runtime::OperatingSystem::Linux.to_string(), "linux");
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatingSystem {
    /// Apple macOS.
    Macos,
    /// GNU/Linux.
    Linux,
}

impl fmt::Display for OperatingSystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Macos => formatter.write_str("macos"),
            Self::Linux => formatter.write_str("linux"),
        }
    }
}

/// A CPU architecture supported by the native runtime.
///
/// ```
/// assert_eq!(nwnrs_runtime::Architecture::Aarch64.to_string(), "aarch64");
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Architecture {
    /// The 64-bit ARM architecture.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// The 64-bit x86 architecture.
    #[serde(rename = "x86_64")]
    X86_64,
}

impl fmt::Display for Architecture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aarch64 => formatter.write_str("aarch64"),
            Self::X86_64 => formatter.write_str("x86_64"),
        }
    }
}

/// A supported operating-system and CPU-architecture pair.
///
/// ```
/// use nwnrs_runtime::{Architecture, OperatingSystem, Platform};
/// let platform = Platform {
///     os: OperatingSystem::Linux,
///     architecture: Architecture::X86_64,
/// };
/// assert_eq!(platform.to_string(), "linux-x86_64");
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    /// The executable operating system.
    pub os:           OperatingSystem,
    /// The executable CPU architecture.
    pub architecture: Architecture,
}

impl Platform {
    /// Returns the platform on which this crate was compiled.
    ///
    /// # Errors
    ///
    /// Returns an error when compiled for an unsupported operating system or
    /// architecture.
    ///
    /// ```
    /// let platform = nwnrs_runtime::Platform::host()?;
    /// assert!(!platform.directory_name().is_empty());
    /// # Ok::<(), nwnrs_runtime::RuntimeError>(())
    /// ```
    pub fn host() -> RuntimeResult<Self> {
        let os = if cfg!(target_os = "macos") {
            OperatingSystem::Macos
        } else if cfg!(target_os = "linux") {
            OperatingSystem::Linux
        } else {
            return Err(RuntimeError::new(format!(
                "unsupported host operating system: {}",
                env::consts::OS
            )));
        };

        let architecture = if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else if cfg!(target_arch = "x86_64") {
            Architecture::X86_64
        } else {
            return Err(RuntimeError::new(format!(
                "unsupported host architecture: {}",
                env::consts::ARCH
            )));
        };

        Ok(Self {
            os,
            architecture,
        })
    }

    /// Returns the target-pack directory component for this platform.
    ///
    /// ```
    /// use nwnrs_runtime::{Architecture, OperatingSystem, Platform};
    /// let platform = Platform {
    ///     os: OperatingSystem::Macos,
    ///     architecture: Architecture::Aarch64,
    /// };
    /// assert_eq!(platform.directory_name(), "macos-aarch64");
    /// ```
    #[must_use]
    pub fn directory_name(self) -> String {
        format!("{}-{}", self.os, self.architecture)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}-{}", self.os, self.architecture)
    }
}

/// The stable SHA-256 identity of one file.
///
/// ```
/// let digest: Option<nwnrs_runtime::FileSha256> = None;
/// assert!(digest.is_none());
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileSha256([u8; 32]);

impl fmt::Display for FileSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The identity and platform encoded by one native executable file.
///
/// ```no_run
/// let identity = nwnrs_runtime::BinaryIdentity::read(std::env::current_exe()?)?;
/// assert_eq!(identity.sha256.to_string().len(), 64);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryIdentity {
    /// Canonical path to the binary.
    pub path:     PathBuf,
    /// SHA-256 of the complete binary file.
    pub sha256:   FileSha256,
    /// Platform encoded in the ELF or Mach-O header.
    pub platform: Platform,
}

impl BinaryIdentity {
    /// Reads and identifies an ELF or Mach-O binary.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be canonicalized or read, or when
    /// its binary format, architecture, or operating system is unsupported.
    ///
    /// ```no_run
    /// let identity = nwnrs_runtime::BinaryIdentity::read("/path/to/nwserver")?;
    /// assert_eq!(identity.sha256.to_string().len(), 64);
    /// # Ok::<(), nwnrs_runtime::RuntimeError>(())
    /// ```
    pub fn read(path: impl AsRef<Path>) -> RuntimeResult<Self> {
        let requested = path.as_ref();
        let path = fs::canonicalize(requested).map_err(|error| {
            RuntimeError::new(format!(
                "failed to resolve binary {}: {error}",
                requested.display()
            ))
        })?;
        let mut file = File::open(&path).map_err(|error| {
            RuntimeError::new(format!("failed to open binary {}: {error}", path.display()))
        })?;
        let platform = read_platform(&mut file, &path)?;
        let sha256 = file_sha256(&path)?;

        Ok(Self {
            path,
            sha256,
            platform,
        })
    }
}

/// Metadata binding one target pack to one exact server binary.
///
/// ```
/// use nwnrs_runtime::{Architecture, OperatingSystem, Platform, TargetServer};
/// let server = TargetServer {
///     sha256: "0".repeat(64),
///     platform: Platform {
///         os: OperatingSystem::Linux,
///         architecture: Architecture::X86_64,
///     },
///     build: None,
/// };
/// assert_eq!(server.sha256.len(), 64);
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetServer {
    /// Complete lowercase SHA-256 of the server binary.
    pub sha256:   String,
    /// Operating system and architecture expected by the hook definitions.
    pub platform: Platform,
    /// Human-readable server build associated with this exact binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build:    Option<String>,
}

/// One address resolved within the main server executable.
///
/// Symbols are useful when the executable retains a trustworthy symbol table.
/// Module-relative offsets remain available for stripped executables.
///
/// ```
/// let address = nwnrs_runtime::TargetAddress::Symbol {
///     symbol: "engine_symbol".to_string(),
/// };
/// assert!(matches!(address, nwnrs_runtime::TargetAddress::Symbol { .. }));
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, untagged)]
pub enum TargetAddress {
    /// Resolve a symbol by its exact native name.
    Symbol {
        /// Exact symbol name recorded by the target pack.
        symbol: String,
    },
    /// Add an offset to the main executable module's load address.
    Offset {
        /// Module-relative virtual address.
        offset: u64,
    },
}

/// Exact engine entry points required by the initial NWScript bridge.
///
/// ```
/// let target: Option<nwnrs_runtime::BridgeTarget> = None;
/// assert!(target.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeTarget {
    /// `CNWSVirtualMachineCommands::ExecuteCommandNWNXFunctionManagement`.
    pub function_management:    TargetAddress,
    /// Byte offset of `CVirtualMachineCmdImplementer::m_pVM` from the command
    /// implementer pointer received by the hook.
    pub virtual_machine_offset: u64,
    /// `CVirtualMachine::StackPopInteger`.
    pub stack_pop_integer:      TargetAddress,
    /// `CVirtualMachine::StackPushInteger`.
    pub stack_push_integer:     TargetAddress,
    /// `CVirtualMachine::StackPopFloat`.
    pub stack_pop_float:        TargetAddress,
    /// `CVirtualMachine::StackPushFloat`.
    pub stack_push_float:       TargetAddress,
    /// `CVirtualMachine::StackPopObject`.
    pub stack_pop_object:       TargetAddress,
    /// `CVirtualMachine::StackPushObject`.
    pub stack_push_object:      TargetAddress,
    /// `CVirtualMachine::StackPopString`.
    pub stack_pop_string:       TargetAddress,
    /// `CVirtualMachine::StackPushString`.
    pub stack_push_string:      TargetAddress,
    /// `CVirtualMachine::StackPopVector`.
    pub stack_pop_vector:       TargetAddress,
    /// `CVirtualMachine::StackPushVector`.
    pub stack_push_vector:      TargetAddress,
    /// Array deallocator used by `CExoString::Clear`, normally `operator
    /// delete[](void*)` on the supported C++ runtimes.
    pub free_exo_string_buffer: TargetAddress,
}

/// Exact engine entry points and layouts used to read live server state.
///
/// ```
/// let target: Option<nwnrs_runtime::ServerStateTarget> = None;
/// assert!(target.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerStateTarget {
    /// Address of the global `CAppManager*` storage.
    pub app_manager:                    TargetAddress,
    /// Byte offset of `CAppManager::m_pServerExoApp`.
    pub server_exo_app_offset:          u64,
    /// `CServerExoApp::GetServerInfo`.
    pub get_server_info:                TargetAddress,
    /// Byte offset of `CServerInfo::m_sModuleName`.
    pub server_info_module_name_offset: u64,
    /// `CServerExoApp::GetPlayerList`.
    pub get_player_list:                TargetAddress,
    /// Byte offset of `CExoArrayList::num`.
    pub player_list_count_offset:       u64,
    /// `CServerExoApp::GetNetLayer`.
    pub get_net_layer:                  TargetAddress,
    /// `CNetLayer::GetSessionMaxPlayers`.
    pub get_session_max_players:        TargetAddress,
}

/// Exact virtual-machine layouts used to read the active event script.
///
/// ```
/// let target = nwnrs_runtime::EventTarget {
///     recursion_level_offset: 36,
///     script_array_offset: 40,
///     script_slot_count: 8,
///     script_stride: 152,
///     script_name_offset: 24,
///     script_event_id_offset: 72,
/// };
/// assert_eq!(target.script_slot_count, 8);
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventTarget {
    /// Byte offset of `CVirtualMachine::m_nRecursionLevel`.
    pub recursion_level_offset: u64,
    /// Byte offset of `CVirtualMachine::m_pVirtualMachineScript`.
    pub script_array_offset:    u64,
    /// Number of `CVirtualMachineScript` slots.
    pub script_slot_count:      u32,
    /// Size in bytes of one `CVirtualMachineScript`.
    pub script_stride:          u64,
    /// Byte offset of `CVirtualMachineScript::m_sScriptName`.
    pub script_name_offset:     u64,
    /// Byte offset of `CVirtualMachineScript::m_nScriptEventID`.
    pub script_event_id_offset: u64,
}

/// Versioned runtime metadata for one exact server binary.
///
/// ```
/// use nwnrs_runtime::{
///     Architecture, OperatingSystem, Platform, TargetPack, TargetServer,
///     RUNTIME_API_VERSION, TARGET_PACK_SCHEMA_VERSION,
/// };
/// let pack = TargetPack {
///     schema_version: TARGET_PACK_SCHEMA_VERSION,
///     runtime_api: RUNTIME_API_VERSION,
///     server: TargetServer {
///         sha256: "0".repeat(64),
///         platform: Platform {
///             os: OperatingSystem::Linux,
///             architecture: Architecture::Aarch64,
///         },
///         build: Some("fixture".to_string()),
///     },
///     bridge: nwnrs_runtime::BridgeTarget {
///         function_management: nwnrs_runtime::TargetAddress::Offset { offset: 1 },
///         virtual_machine_offset: 16,
///         stack_pop_integer: nwnrs_runtime::TargetAddress::Offset { offset: 2 },
///         stack_push_integer: nwnrs_runtime::TargetAddress::Offset { offset: 3 },
///         stack_pop_float: nwnrs_runtime::TargetAddress::Offset { offset: 4 },
///         stack_push_float: nwnrs_runtime::TargetAddress::Offset { offset: 5 },
///         stack_pop_object: nwnrs_runtime::TargetAddress::Offset { offset: 6 },
///         stack_push_object: nwnrs_runtime::TargetAddress::Offset { offset: 7 },
///         stack_pop_string: nwnrs_runtime::TargetAddress::Offset { offset: 8 },
///         stack_push_string: nwnrs_runtime::TargetAddress::Offset { offset: 9 },
///         stack_pop_vector: nwnrs_runtime::TargetAddress::Offset { offset: 10 },
///         stack_push_vector: nwnrs_runtime::TargetAddress::Offset { offset: 11 },
///         free_exo_string_buffer: nwnrs_runtime::TargetAddress::Offset { offset: 12 },
///     },
///     server_state: nwnrs_runtime::ServerStateTarget {
///         app_manager: nwnrs_runtime::TargetAddress::Offset { offset: 13 },
///         server_exo_app_offset: 8,
///         get_server_info: nwnrs_runtime::TargetAddress::Offset { offset: 14 },
///         server_info_module_name_offset: 8,
///         get_player_list: nwnrs_runtime::TargetAddress::Offset { offset: 15 },
///         player_list_count_offset: 8,
///         get_net_layer: nwnrs_runtime::TargetAddress::Offset { offset: 16 },
///         get_session_max_players: nwnrs_runtime::TargetAddress::Offset { offset: 17 },
///     },
///     events: nwnrs_runtime::EventTarget {
///         recursion_level_offset: 36,
///         script_array_offset: 40,
///         script_slot_count: 8,
///         script_stride: 152,
///         script_name_offset: 24,
///         script_event_id_offset: 72,
///     },
/// };
/// assert_eq!(pack.runtime_api, 2);
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPack {
    /// Version of the target-pack file schema.
    pub schema_version: u32,
    /// Exact runtime API version required by this target pack.
    pub runtime_api:    u32,
    /// Exact server identity associated with this pack.
    pub server:         TargetServer,
    /// Minimal native ABI required by the NWScript bridge.
    pub bridge:         BridgeTarget,
    /// Exact native ABI required to read live server state.
    pub server_state:   ServerStateTarget,
    /// Exact native ABI required to observe existing event scripts.
    pub events:         EventTarget,
}

/// A loaded target pack and its canonical source path.
///
/// ```
/// let selected: Option<nwnrs_runtime::SelectedTargetPack> = None;
/// assert!(selected.is_none());
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedTargetPack {
    /// Canonical path to the selected pack.
    pub path: PathBuf,
    /// Parsed and validated target-pack metadata.
    pub pack: TargetPack,
}

/// Validated configuration for one injected runtime process.
///
/// ```
/// let context: Option<nwnrs_runtime::RuntimeContext> = None;
/// assert!(context.is_none());
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeContext {
    /// Identity of the current process executable.
    pub server:   BinaryIdentity,
    /// Exact target pack selected for the executable.
    pub target:   SelectedTargetPack,
    /// Whether initialization failures must terminate the process.
    pub required: bool,
}

/// Loads and validates an exact target-pack file.
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed, its schema is
/// incompatible, or its server identity does not match `binary`.
///
/// ```no_run
/// let binary = nwnrs_runtime::BinaryIdentity::read("/path/to/nwserver")?;
/// let target = nwnrs_runtime::load_target_pack("target.toml", &binary)?;
/// assert_eq!(target.pack.server.sha256, binary.sha256.to_string());
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn load_target_pack(
    path: impl AsRef<Path>,
    binary: &BinaryIdentity,
) -> RuntimeResult<SelectedTargetPack> {
    let requested = path.as_ref();
    let path = fs::canonicalize(requested).map_err(|error| {
        RuntimeError::new(format!(
            "failed to resolve target pack {}: {error}",
            requested.display()
        ))
    })?;
    let text = fs::read_to_string(&path).map_err(|error| {
        RuntimeError::new(format!(
            "failed to read target pack {}: {error}",
            path.display()
        ))
    })?;
    let pack = toml::from_str::<TargetPack>(&text).map_err(|error| {
        RuntimeError::new(format!(
            "failed to parse target pack {}: {error}",
            path.display()
        ))
    })?;
    validate_target_pack(&pack, binary)?;
    Ok(SelectedTargetPack {
        path,
        pack,
    })
}

/// Resolves a target pack from an exact server identity.
///
/// # Errors
///
/// Returns an error when the derived pack does not exist or fails validation.
///
/// ```no_run
/// let binary = nwnrs_runtime::BinaryIdentity::read("/path/to/nwserver")?;
/// let target = nwnrs_runtime::resolve_target_pack("crates/runtime/targets", &binary)?;
/// assert_eq!(target.pack.server.platform, binary.platform);
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn resolve_target_pack(
    target_root: impl AsRef<Path>,
    binary: &BinaryIdentity,
) -> RuntimeResult<SelectedTargetPack> {
    let path = target_root
        .as_ref()
        .join(binary.platform.directory_name())
        .join(format!("{}.toml", binary.sha256));
    load_target_pack(path, binary)
}

/// Initializes configuration for the current injected process from its
/// environment.
///
/// `NWNRS_ENABLED=1` is required. When it is absent, this returns `Ok(None)` so
/// linking the runtime crate into tests or tools has no process-wide effect.
///
/// # Errors
///
/// Returns an error when the current executable cannot be identified, no
/// target location is configured, or target-pack validation fails.
///
/// ```no_run
/// let context = nwnrs_runtime::initialize_current_process()?;
/// if let Some(context) = context {
///     assert_eq!(context.server.platform, context.target.pack.server.platform);
/// }
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn initialize_current_process() -> RuntimeResult<Option<RuntimeContext>> {
    if env::var_os(ENV_ENABLED).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Ok(None);
    }

    let required = env::var_os(ENV_REQUIRED).as_deref() == Some(std::ffi::OsStr::new("1"));
    let executable = env::current_exe().map_err(|error| {
        RuntimeError::new(format!("failed to locate current executable: {error}"))
    })?;
    let server = BinaryIdentity::read(executable)?;
    let target = if let Some(path) = env::var_os(ENV_TARGET_PACK) {
        load_target_pack(path, &server)?
    } else if let Some(path) = env::var_os(ENV_TARGET_DIR) {
        resolve_target_pack(path, &server)?
    } else {
        return Err(RuntimeError::new(format!(
            "neither {ENV_TARGET_PACK} nor {ENV_TARGET_DIR} is configured"
        )));
    };

    Ok(Some(RuntimeContext {
        server,
        target,
        required,
    }))
}

fn validate_target_pack(pack: &TargetPack, binary: &BinaryIdentity) -> RuntimeResult<()> {
    if pack.schema_version != TARGET_PACK_SCHEMA_VERSION {
        return Err(RuntimeError::new(format!(
            "unsupported target-pack schema {}; expected {TARGET_PACK_SCHEMA_VERSION}",
            pack.schema_version
        )));
    }
    if pack.runtime_api != RUNTIME_API_VERSION {
        return Err(RuntimeError::new(format!(
            "target pack requires runtime API {}; this runtime implements {RUNTIME_API_VERSION}",
            pack.runtime_api
        )));
    }
    if pack.server.platform != binary.platform {
        return Err(RuntimeError::new(format!(
            "target pack platform {} does not match binary platform {}",
            pack.server.platform, binary.platform
        )));
    }
    let actual_sha256 = binary.sha256.to_string();
    if !is_sha256(&pack.server.sha256) {
        return Err(RuntimeError::new(
            "target pack server.sha256 must contain 64 lowercase hexadecimal characters",
        ));
    }
    if pack.server.sha256 != actual_sha256 {
        return Err(RuntimeError::new(format!(
            "target pack server SHA-256 {} does not match binary SHA-256 {actual_sha256}",
            pack.server.sha256
        )));
    }
    if !pack.bridge.virtual_machine_offset.is_multiple_of(8) {
        return Err(RuntimeError::new(
            "target pack bridge.virtual_machine_offset must be eight-byte aligned",
        ));
    }
    for (name, address) in bridge_addresses(&pack.bridge) {
        validate_target_address("bridge", name, address)?;
    }
    for (name, address) in server_state_addresses(&pack.server_state) {
        validate_target_address("server_state", name, address)?;
    }
    for (name, offset) in [
        (
            "server_exo_app_offset",
            pack.server_state.server_exo_app_offset,
        ),
        (
            "server_info_module_name_offset",
            pack.server_state.server_info_module_name_offset,
        ),
        (
            "player_list_count_offset",
            pack.server_state.player_list_count_offset,
        ),
    ] {
        if !offset.is_multiple_of(8) {
            return Err(RuntimeError::new(format!(
                "target pack server_state.{name} must be eight-byte aligned"
            )));
        }
    }
    if !pack.events.recursion_level_offset.is_multiple_of(4) {
        return Err(RuntimeError::new(
            "target pack events.recursion_level_offset must be four-byte aligned",
        ));
    }
    for (name, offset) in [
        ("script_array_offset", pack.events.script_array_offset),
        ("script_stride", pack.events.script_stride),
        ("script_name_offset", pack.events.script_name_offset),
    ] {
        if !offset.is_multiple_of(8) {
            return Err(RuntimeError::new(format!(
                "target pack events.{name} must be eight-byte aligned"
            )));
        }
    }
    if !pack.events.script_event_id_offset.is_multiple_of(4) {
        return Err(RuntimeError::new(
            "target pack events.script_event_id_offset must be four-byte aligned",
        ));
    }
    if pack.events.script_slot_count == 0 {
        return Err(RuntimeError::new(
            "target pack events.script_slot_count must be greater than zero",
        ));
    }
    if pack.events.script_name_offset.saturating_add(16) > pack.events.script_stride
        || pack.events.script_event_id_offset.saturating_add(4) > pack.events.script_stride
    {
        return Err(RuntimeError::new(
            "target pack event script fields exceed events.script_stride",
        ));
    }
    Ok(())
}

fn validate_target_address(
    section: &str,
    name: &str,
    address: &TargetAddress,
) -> RuntimeResult<()> {
    if let TargetAddress::Symbol {
        symbol,
    } = address
        && (symbol.is_empty() || symbol.as_bytes().contains(&0))
    {
        return Err(RuntimeError::new(format!(
            "target pack {section}.{name} symbol must be non-empty and contain no NUL bytes"
        )));
    }
    Ok(())
}

fn bridge_addresses(bridge: &BridgeTarget) -> [(&'static str, &TargetAddress); 12] {
    [
        ("function_management", &bridge.function_management),
        ("stack_pop_integer", &bridge.stack_pop_integer),
        ("stack_push_integer", &bridge.stack_push_integer),
        ("stack_pop_float", &bridge.stack_pop_float),
        ("stack_push_float", &bridge.stack_push_float),
        ("stack_pop_object", &bridge.stack_pop_object),
        ("stack_push_object", &bridge.stack_push_object),
        ("stack_pop_string", &bridge.stack_pop_string),
        ("stack_push_string", &bridge.stack_push_string),
        ("stack_pop_vector", &bridge.stack_pop_vector),
        ("stack_push_vector", &bridge.stack_push_vector),
        ("free_exo_string_buffer", &bridge.free_exo_string_buffer),
    ]
}

fn server_state_addresses(server_state: &ServerStateTarget) -> [(&'static str, &TargetAddress); 5] {
    [
        ("app_manager", &server_state.app_manager),
        ("get_server_info", &server_state.get_server_info),
        ("get_player_list", &server_state.get_player_list),
        ("get_net_layer", &server_state.get_net_layer),
        (
            "get_session_max_players",
            &server_state.get_session_max_players,
        ),
    ]
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn file_sha256(path: &Path) -> RuntimeResult<FileSha256> {
    let mut file = File::open(path).map_err(|error| {
        RuntimeError::new(format!("failed to hash binary {}: {error}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            RuntimeError::new(format!("failed to hash binary {}: {error}", path.display()))
        })?;
        if count == 0 {
            break;
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            RuntimeError::new("file reader returned a byte count larger than its buffer")
        })?;
        hasher.update(chunk);
    }

    let digest = hasher.finalize();
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Ok(FileSha256(bytes))
}

fn read_platform(file: &mut File, path: &Path) -> RuntimeResult<Platform> {
    let mut prefix = [0_u8; 8];
    file.read_exact(&mut prefix).map_err(|error| {
        RuntimeError::new(format!(
            "failed to read binary header {}: {error}",
            path.display()
        ))
    })?;
    let magic = prefix
        .get(..4)
        .ok_or_else(|| RuntimeError::new("binary header is shorter than four bytes"))?;
    let header_length = if matches!(magic, b"\xca\xfe\xba\xbe" | b"\xca\xfe\xba\xbf") {
        let count = read_u32_be(&prefix, 4, "Mach-O architecture count")?;
        if count == 0 || count > 64 {
            return Err(RuntimeError::new(format!(
                "unsupported Mach-O architecture count: {count}"
            )));
        }
        let entry_size = if magic == b"\xca\xfe\xba\xbf" { 32 } else { 20 };
        8_usize
            .checked_add(
                usize::try_from(count)
                    .map_err(|_error| RuntimeError::new("Mach-O architecture count overflowed"))?
                    .checked_mul(entry_size)
                    .ok_or_else(|| RuntimeError::new("Mach-O architecture table overflowed"))?,
            )
            .ok_or_else(|| RuntimeError::new("Mach-O header length overflowed"))?
    } else {
        64
    };
    let mut header = vec![0_u8; header_length];
    let prefix_target = header
        .get_mut(..prefix.len())
        .ok_or_else(|| RuntimeError::new("invalid binary header length"))?;
    prefix_target.copy_from_slice(&prefix);
    file.read_exact(
        header
            .get_mut(prefix.len()..)
            .ok_or_else(|| RuntimeError::new("invalid binary header remainder"))?,
    )
    .map_err(|error| {
        RuntimeError::new(format!(
            "failed to read binary header {}: {error}",
            path.display()
        ))
    })?;
    parse_platform(&header)
}

fn parse_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let magic = header
        .get(..4)
        .ok_or_else(|| RuntimeError::new("binary header is shorter than four bytes"))?;
    if magic == b"\x7fELF" {
        return parse_elf_platform(header);
    }
    if magic == b"\xcf\xfa\xed\xfe" {
        return parse_macho_platform(header);
    }
    if matches!(magic, b"\xca\xfe\xba\xbe" | b"\xca\xfe\xba\xbf") {
        return parse_universal_macho_platform(header);
    }
    Err(RuntimeError::new(
        "unsupported binary format; expected 64-bit ELF or little-endian Mach-O",
    ))
}

fn parse_universal_macho_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let magic = header
        .get(..4)
        .ok_or_else(|| RuntimeError::new("Mach-O header is missing its magic"))?;
    let entry_size = if magic == b"\xca\xfe\xba\xbf" { 32 } else { 20 };
    let count = read_u32_be(header, 4, "Mach-O architecture count")?;
    let host_architecture = if cfg!(target_arch = "aarch64") {
        Architecture::Aarch64
    } else if cfg!(target_arch = "x86_64") {
        Architecture::X86_64
    } else {
        return Err(RuntimeError::new(format!(
            "unsupported host architecture: {}",
            env::consts::ARCH
        )));
    };

    for index in 0..count {
        let index = usize::try_from(index)
            .map_err(|_error| RuntimeError::new("Mach-O architecture index overflowed"))?;
        let offset = 8_usize
            .checked_add(
                index
                    .checked_mul(entry_size)
                    .ok_or_else(|| RuntimeError::new("Mach-O architecture offset overflowed"))?,
            )
            .ok_or_else(|| RuntimeError::new("Mach-O architecture offset overflowed"))?;
        let cpu_type = read_u32_be(header, offset, "Mach-O universal CPU type")?;
        let architecture = match cpu_type {
            0x0100_000c => Architecture::Aarch64,
            0x0100_0007 => Architecture::X86_64,
            _ => continue,
        };
        if architecture == host_architecture {
            return Ok(Platform {
                os: OperatingSystem::Macos,
                architecture,
            });
        }
    }
    Err(RuntimeError::new(format!(
        "universal Mach-O does not contain the host architecture {host_architecture}"
    )))
}

fn parse_elf_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let class = header
        .get(4)
        .copied()
        .ok_or_else(|| RuntimeError::new("ELF header is missing its class"))?;
    let encoding = header
        .get(5)
        .copied()
        .ok_or_else(|| RuntimeError::new("ELF header is missing its byte order"))?;
    if class != 2 || encoding != 1 {
        return Err(RuntimeError::new(
            "only little-endian 64-bit ELF binaries are supported",
        ));
    }
    let machine = read_u16_le(header, 18, "ELF machine")?;
    let architecture = match machine {
        62 => Architecture::X86_64,
        183 => Architecture::Aarch64,
        _ => {
            return Err(RuntimeError::new(format!(
                "unsupported ELF machine identifier: {machine}"
            )));
        }
    };
    Ok(Platform {
        os: OperatingSystem::Linux,
        architecture,
    })
}

fn parse_macho_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let cpu_type = read_u32_le(header, 4, "Mach-O CPU type")?;
    let architecture = match cpu_type {
        0x0100_000c => Architecture::Aarch64,
        0x0100_0007 => Architecture::X86_64,
        _ => {
            return Err(RuntimeError::new(format!(
                "unsupported Mach-O CPU type: {cpu_type:#010x}"
            )));
        }
    };
    Ok(Platform {
        os: OperatingSystem::Macos,
        architecture,
    })
}

fn read_u16_le(bytes: &[u8], offset: usize, field: &str) -> RuntimeResult<u16> {
    let value = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| RuntimeError::new(format!("binary header is missing {field}")))?;
    let array = <[u8; 2]>::try_from(value)
        .map_err(|_error| RuntimeError::new(format!("invalid {field} width")))?;
    Ok(u16::from_le_bytes(array))
}

fn read_u32_le(bytes: &[u8], offset: usize, field: &str) -> RuntimeResult<u32> {
    let value = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| RuntimeError::new(format!("binary header is missing {field}")))?;
    let array = <[u8; 4]>::try_from(value)
        .map_err(|_error| RuntimeError::new(format!("invalid {field} width")))?;
    Ok(u32::from_le_bytes(array))
}

fn read_u32_be(bytes: &[u8], offset: usize, field: &str) -> RuntimeResult<u32> {
    let value = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| RuntimeError::new(format!("binary header is missing {field}")))?;
    let array = <[u8; 4]>::try_from(value)
        .map_err(|_error| RuntimeError::new(format!("invalid {field} width")))?;
    Ok(u32::from_be_bytes(array))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::{
        Architecture, BinaryIdentity, BridgeTarget, EventTarget, OperatingSystem, Platform,
        RUNTIME_API_VERSION, ServerStateTarget, TARGET_PACK_SCHEMA_VERSION, TargetAddress,
        TargetPack, TargetServer, bridge_addresses, parse_platform, resolve_target_pack,
        server_state_addresses,
    };

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn parses_supported_elf_and_macho_headers() -> Result<(), Box<dyn std::error::Error>> {
        let mut elf = [0_u8; 64];
        elf.get_mut(..4)
            .ok_or("ELF magic range")?
            .copy_from_slice(b"\x7fELF");
        *elf.get_mut(4).ok_or("ELF class byte")? = 2;
        *elf.get_mut(5).ok_or("ELF byte-order byte")? = 1;
        elf.get_mut(18..20)
            .ok_or("ELF machine range")?
            .copy_from_slice(&62_u16.to_le_bytes());
        assert_eq!(
            parse_platform(&elf)?,
            Platform {
                os:           OperatingSystem::Linux,
                architecture: Architecture::X86_64,
            }
        );

        let mut macho = [0_u8; 32];
        macho
            .get_mut(..4)
            .ok_or("Mach-O magic range")?
            .copy_from_slice(b"\xcf\xfa\xed\xfe");
        macho
            .get_mut(4..8)
            .ok_or("Mach-O CPU range")?
            .copy_from_slice(&0x0100_000c_u32.to_le_bytes());
        assert_eq!(
            parse_platform(&macho)?,
            Platform {
                os:           OperatingSystem::Macos,
                architecture: Architecture::Aarch64,
            }
        );

        let mut universal = [0_u8; 48];
        universal
            .get_mut(..8)
            .ok_or("Mach-O universal prefix")?
            .copy_from_slice(b"\xca\xfe\xba\xbe\0\0\0\x02");
        universal
            .get_mut(8..12)
            .ok_or("Mach-O x86 CPU range")?
            .copy_from_slice(&0x0100_0007_u32.to_be_bytes());
        universal
            .get_mut(28..32)
            .ok_or("Mach-O ARM CPU range")?
            .copy_from_slice(&0x0100_000c_u32.to_be_bytes());
        let expected_architecture = if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else {
            Architecture::X86_64
        };
        assert_eq!(
            parse_platform(&universal)?,
            Platform {
                os:           OperatingSystem::Macos,
                architecture: expected_architecture,
            }
        );
        Ok(())
    }

    #[test]
    fn resolves_only_the_exact_hash_pack() -> Result<(), Box<dyn std::error::Error>> {
        let root = test_directory();
        fs::create_dir_all(&root)?;
        let binary_path = root.join("nwserver");
        let mut elf = [0_u8; 64];
        elf.get_mut(..4)
            .ok_or("ELF magic range")?
            .copy_from_slice(b"\x7fELF");
        *elf.get_mut(4).ok_or("ELF class byte")? = 2;
        *elf.get_mut(5).ok_or("ELF byte-order byte")? = 1;
        elf.get_mut(18..20)
            .ok_or("ELF machine range")?
            .copy_from_slice(&183_u16.to_le_bytes());
        fs::write(&binary_path, elf)?;
        let identity = BinaryIdentity::read(&binary_path)?;
        let mut pack = TargetPack {
            schema_version: TARGET_PACK_SCHEMA_VERSION,
            runtime_api:    RUNTIME_API_VERSION,
            server:         TargetServer {
                sha256:   identity.sha256.to_string(),
                platform: identity.platform,
                build:    Some("fixture".to_string()),
            },
            bridge:         fixture_bridge_target(),
            server_state:   fixture_server_state_target(),
            events:         fixture_event_target(),
        };
        let pack_directory = root.join(identity.platform.directory_name());
        fs::create_dir_all(&pack_directory)?;
        let pack_path = pack_directory.join(format!("{}.toml", identity.sha256));
        fs::write(&pack_path, toml::to_string(&pack)?)?;

        let selected = resolve_target_pack(&root, &identity)?;
        assert_eq!(selected.pack, pack);
        assert_eq!(selected.path, fs::canonicalize(pack_path)?);

        pack.server.sha256 = "0".repeat(64);
        fs::write(&selected.path, toml::to_string(&pack)?)?;
        assert!(resolve_target_pack(&root, &identity).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn source_controlled_target_packs_match_their_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("targets");
        let mut pack_count = 0_usize;
        for platform_entry in fs::read_dir(&root)? {
            let platform_entry = platform_entry?;
            if !platform_entry.file_type()?.is_dir() {
                continue;
            }
            let platform_name = platform_entry
                .file_name()
                .to_str()
                .ok_or("target platform directory is not UTF-8")?
                .to_string();
            for pack_entry in fs::read_dir(platform_entry.path())? {
                let pack_entry = pack_entry?;
                if pack_entry
                    .path()
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    != Some("toml")
                {
                    continue;
                }
                let pack = toml::from_str::<TargetPack>(&fs::read_to_string(pack_entry.path())?)?;
                let filename = pack_entry
                    .path()
                    .file_stem()
                    .and_then(std::ffi::OsStr::to_str)
                    .ok_or("target pack filename is not UTF-8")?
                    .to_string();
                assert_eq!(pack.schema_version, TARGET_PACK_SCHEMA_VERSION);
                assert_eq!(pack.runtime_api, RUNTIME_API_VERSION);
                assert_eq!(pack.server.sha256, filename);
                assert_eq!(pack.server.platform.directory_name(), platform_name);
                for (_name, address) in bridge_addresses(&pack.bridge) {
                    if let TargetAddress::Symbol {
                        symbol,
                    } = address
                    {
                        assert!(!symbol.is_empty());
                        assert!(!symbol.as_bytes().contains(&0));
                    }
                }
                for (_name, address) in server_state_addresses(&pack.server_state) {
                    if let TargetAddress::Symbol {
                        symbol,
                    } = address
                    {
                        assert!(!symbol.is_empty());
                        assert!(!symbol.as_bytes().contains(&0));
                    }
                }
                pack_count = pack_count.saturating_add(1);
            }
        }
        assert!(pack_count >= 3);
        Ok(())
    }

    fn fixture_bridge_target() -> BridgeTarget {
        BridgeTarget {
            function_management:    TargetAddress::Offset {
                offset: 1
            },
            virtual_machine_offset: 0,
            stack_pop_integer:      TargetAddress::Offset {
                offset: 2
            },
            stack_push_integer:     TargetAddress::Offset {
                offset: 3
            },
            stack_pop_float:        TargetAddress::Offset {
                offset: 4
            },
            stack_push_float:       TargetAddress::Offset {
                offset: 5
            },
            stack_pop_object:       TargetAddress::Offset {
                offset: 6
            },
            stack_push_object:      TargetAddress::Offset {
                offset: 7
            },
            stack_pop_string:       TargetAddress::Offset {
                offset: 8
            },
            stack_push_string:      TargetAddress::Offset {
                offset: 9
            },
            stack_pop_vector:       TargetAddress::Offset {
                offset: 10
            },
            stack_push_vector:      TargetAddress::Offset {
                offset: 11
            },
            free_exo_string_buffer: TargetAddress::Offset {
                offset: 12
            },
        }
    }

    fn fixture_server_state_target() -> ServerStateTarget {
        ServerStateTarget {
            app_manager:                    TargetAddress::Offset {
                offset: 13
            },
            server_exo_app_offset:          8,
            get_server_info:                TargetAddress::Offset {
                offset: 14
            },
            server_info_module_name_offset: 8,
            get_player_list:                TargetAddress::Offset {
                offset: 15
            },
            player_list_count_offset:       8,
            get_net_layer:                  TargetAddress::Offset {
                offset: 16
            },
            get_session_max_players:        TargetAddress::Offset {
                offset: 17
            },
        }
    }

    fn fixture_event_target() -> EventTarget {
        EventTarget {
            recursion_level_offset: 36,
            script_array_offset:    40,
            script_slot_count:      8,
            script_stride:          152,
            script_name_offset:     24,
            script_event_id_offset: 72,
        }
    }

    fn test_directory() -> PathBuf {
        let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "nwnrs-runtime-test-{}-{sequence}",
            std::process::id()
        ))
    }
}
