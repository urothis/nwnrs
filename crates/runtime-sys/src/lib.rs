#![doc = include_str!("../README.md")]

mod bridge;
mod engine;

use std::{
    env,
    error::Error,
    ffi::c_void,
    fmt,
    io::IsTerminal as _,
    panic::{self, AssertUnwindSafe},
    process, ptr,
    sync::{
        OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
};

pub use bridge::{BridgeInstallError, install_nwscript_bridge};
use nwnrs_runtime::{ENV_REQUIRED, ENV_SUPERVISED, RuntimeContext, initialize_current_process};
use tracing_subscriber::EnvFilter;

static RUNTIME_CONTEXT: OnceLock<RuntimeContext> = OnceLock::new();
static PROBE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

type ProbeFunction = extern "C" fn(i32) -> i32;

/// An error produced by the embedded Frida Gum interception probe.
///
/// ```
/// let error: Option<nwnrs_runtime_sys::ProbeError> = None;
/// assert!(error.is_none());
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeError {
    message: String,
}

impl ProbeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProbeError {}

/// Returns the validated context after successful injected initialization.
///
/// ```
/// assert!(nwnrs_runtime_sys::initialized_context().is_none());
/// ```
#[must_use]
pub fn initialized_context() -> Option<&'static RuntimeContext> {
    RUNTIME_CONTEXT.get()
}

/// Runs an in-process replace, trampoline, and revert probe against Frida Gum.
///
/// This probe does not require a Neverwinter Nights binary or target pack. It
/// is the Phase 0 compatibility gate for each supported build target.
///
/// # Errors
///
/// Returns an error when Gum cannot provide an interceptor, install the
/// replacement, expose the original trampoline, flush its changes, or restore
/// the fixture function.
///
/// ```no_run
/// nwnrs_runtime_sys::run_frida_probe()?;
/// # Ok::<(), nwnrs_runtime_sys::ProbeError>(())
/// ```
pub fn run_frida_probe() -> Result<(), ProbeError> {
    let target: ProbeFunction = probe_target;
    if std::hint::black_box(target)(7) != 8 {
        return Err(ProbeError::new(
            "fixture target returned an unexpected value",
        ));
    }

    // SAFETY: Gum is initialized exactly for the duration of this single-threaded
    // probe, and every acquired object and replacement is released before Gum is
    // deinitialized.
    unsafe {
        frida_gum_sys::gum_init_embedded();
    }
    let probe_result = run_initialized_probe(target);
    // SAFETY: run_initialized_probe reverts the installed replacement and drops
    // the interceptor reference before returning.
    unsafe {
        frida_gum_sys::gum_deinit_embedded();
    }
    probe_result
}

fn run_initialized_probe(target: ProbeFunction) -> Result<(), ProbeError> {
    // SAFETY: Gum has been initialized by run_frida_probe and returns a retained
    // interceptor object or null.
    let interceptor = unsafe { frida_gum_sys::gum_interceptor_obtain() };
    if interceptor.is_null() {
        return Err(ProbeError::new("Frida Gum returned a null interceptor"));
    }

    let target_address = target as *const () as *mut c_void;
    let replacement_address = probe_replacement as ProbeFunction as *const () as *mut c_void;
    let mut original = ptr::null_mut();

    // SAFETY: the target and replacement use the same C ABI and signature;
    // original points to writable storage for Gum's trampoline result.
    let replace_status = unsafe {
        frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
        let status = frida_gum_sys::gum_interceptor_replace(
            interceptor,
            target_address,
            replacement_address,
            ptr::null_mut(),
            &raw mut original,
        );
        frida_gum_sys::gum_interceptor_end_transaction(interceptor);
        status
    };

    if replace_status != 0 {
        release_interceptor(interceptor);
        return Err(ProbeError::new(format!(
            "Frida Gum replacement failed with status {replace_status}"
        )));
    }
    if original.is_null() {
        revert_and_release(interceptor, target_address);
        return Err(ProbeError::new(
            "Frida Gum did not return an original-function trampoline",
        ));
    }

    PROBE_ORIGINAL.store(original, Ordering::Release);
    let replaced_value = std::hint::black_box(target)(7);
    PROBE_ORIGINAL.store(ptr::null_mut(), Ordering::Release);
    revert_and_release(interceptor, target_address);

    if replaced_value != 18 {
        return Err(ProbeError::new(format!(
            "replacement returned {replaced_value}; expected 18"
        )));
    }
    if std::hint::black_box(target)(7) != 8 {
        return Err(ProbeError::new(
            "fixture target was not restored after interceptor revert",
        ));
    }
    Ok(())
}

fn revert_and_release(
    interceptor: *mut frida_gum_sys::GumInterceptor,
    target_address: *mut c_void,
) {
    // SAFETY: interceptor is retained and target_address names the replacement
    // installed by run_initialized_probe.
    unsafe {
        frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
        frida_gum_sys::gum_interceptor_revert(interceptor, target_address);
        frida_gum_sys::gum_interceptor_end_transaction(interceptor);
        let _flushed = frida_gum_sys::gum_interceptor_flush(interceptor);
    }
    release_interceptor(interceptor);
}

fn release_interceptor(interceptor: *mut frida_gum_sys::GumInterceptor) {
    // SAFETY: gum_interceptor_obtain returned one retained GObject reference.
    unsafe {
        frida_gum_sys::g_object_unref(interceptor.cast());
    }
}

#[inline(never)]
extern "C" fn probe_target(value: i32) -> i32 {
    value.saturating_add(1)
}

extern "C" fn probe_replacement(value: i32) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(|| {
        let original = PROBE_ORIGINAL.load(Ordering::Acquire);
        if original.is_null() {
            return i32::MIN;
        }
        // SAFETY: Gum populated this pointer with a trampoline having the exact
        // ProbeFunction signature before PROBE_ORIGINAL was published.
        let original = unsafe { std::mem::transmute::<*mut c_void, ProbeFunction>(original) };
        original(value).saturating_add(10)
    }))
    .unwrap_or(i32::MIN)
}

extern "C" fn initialize_injected_runtime() {
    init_tracing();
    let initialized = panic::catch_unwind(initialize_current_process);
    match initialized {
        Ok(Ok(None)) => {}
        Ok(Ok(Some(context))) => initialize_gum_runtime(context),
        Ok(Err(error)) => initialization_failed(&error.to_string()),
        Err(_payload) => initialization_failed("runtime initialization panicked"),
    }
}

fn initialize_gum_runtime(context: RuntimeContext) {
    // SAFETY: process-loader initialization calls this once before runtime hooks
    // are installed. Gum remains initialized for the process lifetime.
    unsafe {
        frida_gum_sys::gum_init_embedded();
    }
    if RUNTIME_CONTEXT.set(context).is_err() {
        initialization_failed("runtime was initialized more than once");
        return;
    }
    let Some(context) = RUNTIME_CONTEXT.get() else {
        initialization_failed("runtime context was unavailable after initialization");
        return;
    };
    if let Err(error) = install_nwscript_bridge(context) {
        initialization_failed(&error.to_string());
        return;
    }
    tracing::info!(target: "nwnrs::runtime", "initialized");
}

fn initialization_failed(message: &str) {
    write_diagnostic(message);
    if env::var_os(ENV_REQUIRED).as_deref() == Some(std::ffi::OsStr::new("1")) {
        process::exit(70);
    }
}

fn write_diagnostic(message: &str) {
    tracing::error!(target: "nwnrs::runtime", "{message}");
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_error| EnvFilter::new("warn,nwnrs::runtime=info,nwnrs::script=info"));
    let color = env::var_os(ENV_SUPERVISED).is_none()
        && match env::var("NWNRS_COLOR").as_deref() {
            Ok("always") => true,
            Ok("never") => false,
            _ => std::io::stderr().is_terminal() && env::var_os("NO_COLOR").is_none(),
        };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(color)
        .without_time()
        .with_target(true)
        .try_init();
}

#[cfg(target_os = "linux")]
#[used]
#[unsafe(link_section = ".init_array")]
static NWNRS_RUNTIME_INITIALIZER: extern "C" fn() = initialize_injected_runtime;

#[cfg(target_os = "macos")]
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static NWNRS_RUNTIME_INITIALIZER: extern "C" fn() = initialize_injected_runtime;
