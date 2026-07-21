use std::{
    collections::HashMap,
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use crate::bridge::BridgeInstallError;

/// One physical native detour. Logical events sharing an address must be
/// multiplexed by the family replacement behind a single specification.
pub(crate) struct NativeHookSpec {
    name:        &'static str,
    target:      usize,
    replacement: usize,
    original:    &'static AtomicPtr<c_void>,
}

impl NativeHookSpec {
    pub(crate) const fn new(
        name: &'static str,
        target: usize,
        replacement: usize,
        original: &'static AtomicPtr<c_void>,
    ) -> Self {
        Self {
            name,
            target,
            replacement,
            original,
        }
    }
}

pub(crate) fn install_native_hooks(hooks: &[NativeHookSpec]) -> Result<(), BridgeInstallError> {
    let mut targets = HashMap::with_capacity(hooks.len());
    for hook in hooks {
        if let Some(existing) = targets.insert(hook.target, hook.name) {
            return Err(BridgeInstallError::new(format!(
                "native hook target {:#x} is registered by both {existing} and {}",
                hook.target, hook.name
            )));
        }
    }

    // SAFETY: Gum is initialized and returns a retained interceptor or null.
    let interceptor = unsafe { frida_gum_sys::gum_interceptor_obtain() };
    if interceptor.is_null() {
        return Err(BridgeInstallError::new("Frida Gum returned no interceptor"));
    }
    let result = install_with_interceptor(interceptor, hooks);
    // SAFETY: gum_interceptor_obtain returned one retained GObject reference.
    unsafe {
        frida_gum_sys::g_object_unref(interceptor.cast());
    }
    result
}

fn install_with_interceptor(
    interceptor: *mut frida_gum_sys::GumInterceptor,
    hooks: &[NativeHookSpec],
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
