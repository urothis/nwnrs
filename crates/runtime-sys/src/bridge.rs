use std::{
    cell::RefCell,
    error::Error,
    ffi::c_void,
    fmt,
    panic::{self, AssertUnwindSafe},
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
};

use nwnrs_runtime::{
    BridgeError, BridgeErrorCode, BridgeValue, RuntimeContext, ScriptBridge, ScriptLog,
    ScriptLogLevel, Vector,
};

use super::{
    RUNTIME_CONTEXT,
    adapter::NativeRuntimeHost,
    engine::{
        Engine, EngineThreadToken,
        abi::{FunctionManagement, MainLoop, ObjectId},
    },
    write_diagnostic,
};

const VM_SUCCESS: i32 = 0;
const VM_FAKE_ABORT_SCRIPT: i32 = -645;

const NWNX_GET_IS_AVAILABLE: i32 = 1151;
const NWNX_CALL: i32 = 1152;
const NWNX_PUSH_INTEGER: i32 = 1153;
const NWNX_PUSH_FLOAT: i32 = 1154;
const NWNX_PUSH_OBJECT: i32 = 1155;
const NWNX_PUSH_STRING: i32 = 1156;
const NWNX_PUSH_VECTOR: i32 = 1157;
const NWNX_POP_INTEGER: i32 = 1167;
const NWNX_POP_FLOAT: i32 = 1168;
const NWNX_POP_OBJECT: i32 = 1169;
const NWNX_POP_STRING: i32 = 1170;
const NWNX_POP_VECTOR: i32 = 1171;

static ENGINE: OnceLock<Engine> = OnceLock::new();
static FUNCTION_MANAGEMENT_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static MAIN_LOOP_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

thread_local! {
    static SCRIPT_BRIDGE: RefCell<ScriptBridge> = RefCell::new(ScriptBridge::default());
}

/// An error produced while resolving or installing the NWScript bridge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeInstallError {
    message: String,
}

impl BridgeInstallError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BridgeInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BridgeInstallError {}

/// Resolves the exact target-pack ABI and replaces the engine's NWNX command
/// handler with the nwnrs bridge.
///
/// # Errors
///
/// Returns an error when an address cannot be resolved, Gum cannot install the
/// replacement, or the bridge was already initialized.
pub fn install_nwscript_bridge(context: &RuntimeContext) -> Result<(), BridgeInstallError> {
    // SAFETY: Gum was initialized by the injected-runtime initializer.
    let module = unsafe { frida_gum_sys::gum_process_get_main_module() };
    if module.is_null() {
        return Err(BridgeInstallError::new(
            "Frida Gum returned no main executable module",
        ));
    }
    let resolved = Engine::resolve(module, context);
    // SAFETY: Gum returned one retained GObject reference.
    unsafe {
        frida_gum_sys::g_object_unref(module.cast());
    }
    let engine = resolved?;
    let hook_target = engine.hook_target();
    let main_loop_hook_target = engine.administration_main_loop_hook_target();
    if ENGINE.set(engine).is_err() {
        return Err(BridgeInstallError::new(
            "NWScript bridge was initialized more than once",
        ));
    }

    // SAFETY: Gum is initialized and returns a retained interceptor or null.
    let interceptor = unsafe { frida_gum_sys::gum_interceptor_obtain() };
    if interceptor.is_null() {
        return Err(BridgeInstallError::new("Frida Gum returned no interceptor"));
    }
    let bridge_hook = HookSpec {
        name:        "NWScript bridge",
        target:      hook_target,
        replacement: function_management_replacement as FunctionManagement as *const () as usize,
        original:    &FUNCTION_MANAGEMENT_ORIGINAL,
    };
    let mut hooks = vec![bridge_hook];
    if let Some(target) = main_loop_hook_target {
        hooks.push(HookSpec {
            name: "deferred administration",
            target,
            replacement: main_loop_replacement as MainLoop as *const () as usize,
            original: &MAIN_LOOP_ORIGINAL,
        });
    }
    let result = install_hooks(interceptor, &hooks);
    release_interceptor(interceptor);
    result
}

struct HookSpec {
    name:        &'static str,
    target:      usize,
    replacement: usize,
    original:    &'static AtomicPtr<c_void>,
}

fn install_hooks(
    interceptor: *mut frida_gum_sys::GumInterceptor,
    hooks: &[HookSpec],
) -> Result<(), BridgeInstallError> {
    let mut installed = Vec::with_capacity(hooks.len());
    let mut failure = None;
    // SAFETY: target packs bind each exact address to its replacement's ABI.
    unsafe {
        frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
        for hook in hooks {
            let mut original = ptr::null_mut();
            let status = frida_gum_sys::gum_interceptor_replace(
                interceptor,
                hook.target as *mut c_void,
                hook.replacement as *mut c_void,
                ptr::null_mut(),
                &raw mut original,
            );
            if status == 0 && !original.is_null() {
                hook.original.store(original, Ordering::Release);
                installed.push(hook);
            } else {
                failure = Some(format!(
                    "Frida Gum could not install {}: status {status}, trampoline {}",
                    hook.name,
                    if original.is_null() {
                        "missing"
                    } else {
                        "present"
                    }
                ));
                break;
            }
        }
        frida_gum_sys::gum_interceptor_end_transaction(interceptor);
        let _flushed = frida_gum_sys::gum_interceptor_flush(interceptor);
    }
    let Some(failure) = failure else {
        return Ok(());
    };
    // SAFETY: installed contains only replacements added above.
    unsafe {
        frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
        for hook in installed {
            frida_gum_sys::gum_interceptor_revert(interceptor, hook.target as *mut c_void);
            hook.original.store(ptr::null_mut(), Ordering::Release);
        }
        frida_gum_sys::gum_interceptor_end_transaction(interceptor);
        let _flushed = frida_gum_sys::gum_interceptor_flush(interceptor);
    }
    Err(BridgeInstallError::new(failure))
}

fn release_interceptor(interceptor: *mut frida_gum_sys::GumInterceptor) {
    // SAFETY: gum_interceptor_obtain returned one retained GObject reference.
    unsafe {
        frida_gum_sys::g_object_unref(interceptor.cast());
    }
}

pub(crate) extern "C" fn function_management_replacement(
    commands: *mut c_void,
    command: i32,
    parameters: i32,
) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(|| {
        handle_function_management(commands, command, parameters)
    }))
    .unwrap_or(VM_FAKE_ABORT_SCRIPT)
}

pub(crate) extern "C" fn main_loop_replacement(server_internal: *mut c_void) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some(engine) = ENGINE.get() {
            // SAFETY: the replacement runs synchronously on the NWServer main
            // loop thread and does not retain the callback token.
            let thread = unsafe { EngineThreadToken::new() };
            if let Err(error) = engine.process_deferred_administration(&thread) {
                write_diagnostic(&error.to_string());
            }
        }
        call_original_main_loop(server_internal)
    }))
    .unwrap_or_default()
}

fn call_original_main_loop(server_internal: *mut c_void) -> i32 {
    let original = MAIN_LOOP_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the exact MainLoop trampoline before enabling the
    // replacement, and the receiver belongs to this engine callback.
    let original = unsafe { std::mem::transmute::<*mut c_void, MainLoop>(original) };
    original(server_internal)
}

fn handle_function_management(commands: *mut c_void, command: i32, parameters: i32) -> i32 {
    let Some(engine) = ENGINE.get() else {
        return VM_FAKE_ABORT_SCRIPT;
    };
    // SAFETY: this entire scope runs synchronously inside the engine's command
    // callback.
    let thread = unsafe { EngineThreadToken::new() };
    let vm = match engine.virtual_machine(&thread, commands) {
        Ok(vm) => vm,
        Err(error) => {
            write_diagnostic(&error.to_string());
            return VM_FAKE_ABORT_SCRIPT;
        }
    };
    let stack = engine.vm();
    match command {
        NWNX_GET_IS_AVAILABLE => result_code(stack.push_integer(&thread, vm, 1)),
        NWNX_CALL => handle_call(engine, &thread, vm),
        NWNX_PUSH_INTEGER => {
            push_argument(stack.pop_integer(&thread, vm).map(BridgeValue::Integer))
        }
        NWNX_PUSH_FLOAT => push_argument(stack.pop_float(&thread, vm).map(BridgeValue::Float)),
        NWNX_PUSH_OBJECT => push_argument(
            stack
                .pop_object(&thread, vm)
                .map(|value| BridgeValue::Object(value.raw())),
        ),
        NWNX_PUSH_STRING => push_argument(stack.pop_string(&thread, vm).map(BridgeValue::String)),
        NWNX_PUSH_VECTOR => push_argument(stack.pop_vector(&thread, vm).map(BridgeValue::Vector)),
        NWNX_POP_INTEGER => {
            let value = pop_return(ScriptBridge::pop_integer).unwrap_or_default();
            result_code(stack.push_integer(&thread, vm, value))
        }
        NWNX_POP_FLOAT => {
            let value = pop_return(ScriptBridge::pop_float).unwrap_or_default();
            result_code(stack.push_float(&thread, vm, value))
        }
        NWNX_POP_OBJECT => {
            let value = pop_return(ScriptBridge::pop_object)
                .map_or_else(ObjectId::invalid, ObjectId::from_raw);
            result_code(stack.push_object(&thread, vm, value))
        }
        NWNX_POP_STRING => {
            let value = pop_return(ScriptBridge::pop_string).unwrap_or_default();
            result_code(stack.push_string(&thread, vm, &value))
        }
        NWNX_POP_VECTOR => {
            let value = pop_return(ScriptBridge::pop_vector).unwrap_or(Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            });
            result_code(stack.push_vector(&thread, vm, value))
        }
        _ => call_original(commands, command, parameters),
    }
}

fn handle_call(engine: &Engine, thread: &EngineThreadToken, vm: *mut c_void) -> i32 {
    let stack = engine.vm();
    let namespace = match stack.pop_string(thread, vm) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let function = match stack.pop_string(thread, vm) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let namespace = match std::str::from_utf8(&namespace) {
        Ok(value) => value,
        Err(_) => {
            return record_error(BridgeError::new(
                BridgeErrorCode::InvalidArgument,
                "NWScript bridge namespace is not UTF-8",
            ))
        }
    };
    let function = match std::str::from_utf8(&function) {
        Ok(value) => value,
        Err(_) => {
            return record_error(BridgeError::new(
                BridgeErrorCode::InvalidArgument,
                "NWScript bridge function is not UTF-8",
            ))
        }
    };
    let Some(context) = RUNTIME_CONTEXT.get() else {
        return VM_FAKE_ABORT_SCRIPT;
    };
    let mut host = NativeRuntimeHost::new(engine, thread, vm);
    let result = SCRIPT_BRIDGE.try_with(|bridge| {
        let mut bridge = bridge.try_borrow_mut().map_err(|_error| {
            BridgeError::new(
                BridgeErrorCode::Reentrant,
                "NWScript bridge call was reentrant",
            )
        })?;
        bridge.call(namespace, function, context, &mut host)?;
        Ok::<_, BridgeError>(bridge.take_logs())
    });
    match result {
        Ok(Ok(logs)) => {
            for log in logs {
                emit_script_log(&log);
            }
            VM_SUCCESS
        }
        Ok(Err(error)) => {
            write_diagnostic(&error.to_string());
            VM_SUCCESS
        }
        Err(_) => VM_FAKE_ABORT_SCRIPT,
    }
}

fn record_error(error: BridgeError) -> i32 {
    write_diagnostic(&error.to_string());
    let _ = SCRIPT_BRIDGE.try_with(|bridge| {
        if let Ok(mut bridge) = bridge.try_borrow_mut() {
            bridge.record_external_error(error);
        }
    });
    VM_SUCCESS
}

fn emit_script_log(log: &ScriptLog) {
    let message = String::from_utf8_lossy(&log.message);
    match log.level {
        ScriptLogLevel::Trace => tracing::trace!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Debug => tracing::debug!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Info => tracing::info!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Warn => tracing::warn!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Error => tracing::error!(target: "nwnrs::script", "{message}"),
    }
}

fn push_argument(value: Result<BridgeValue, i32>) -> i32 {
    let value = match value {
        Ok(value) => value,
        Err(error) => return error,
    };
    match SCRIPT_BRIDGE.try_with(|bridge| {
        bridge
            .try_borrow_mut()
            .map(|mut bridge| bridge.push_argument(value))
    }) {
        Ok(Ok(())) => VM_SUCCESS,
        _ => VM_FAKE_ABORT_SCRIPT,
    }
}

fn pop_return<T>(
    pop: impl FnOnce(&mut ScriptBridge) -> nwnrs_runtime::BridgeResult<T>,
) -> Option<T> {
    let result = SCRIPT_BRIDGE
        .try_with(|bridge| {
            let mut bridge = bridge.try_borrow_mut().map_err(|_error| {
                BridgeError::new(
                    BridgeErrorCode::Reentrant,
                    "NWScript bridge return pop was reentrant",
                )
            })?;
            pop(&mut bridge)
        })
        .ok()?;
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            write_diagnostic(&error.to_string());
            None
        }
    }
}

fn result_code(result: Result<(), i32>) -> i32 {
    result.map_or_else(|error| error, |()| VM_SUCCESS)
}

fn call_original(commands: *mut c_void, command: i32, parameters: i32) -> i32 {
    let original = FUNCTION_MANAGEMENT_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return VM_FAKE_ABORT_SCRIPT;
    }
    // SAFETY: Gum published the exact FunctionManagement trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, FunctionManagement>(original) };
    original(commands, command, parameters)
}
