use std::{
    collections::HashMap,
    ffi::c_void,
    marker::PhantomData,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use crate::bridge::BridgeInstallError;

/// Typed storage for the trampoline published after one native replacement is
/// installed.
pub(crate) struct NativeOriginal<Function> {
    pointer: AtomicPtr<c_void>,
    marker:  PhantomData<fn() -> Function>,
}

impl<Function> NativeOriginal<Function> {
    pub(crate) const fn new() -> Self {
        Self {
            pointer: AtomicPtr::new(ptr::null_mut()),
            marker:  PhantomData,
        }
    }

    fn storage(&'static self) -> &'static AtomicPtr<c_void> {
        &self.pointer
    }
}

impl<Function: Copy> NativeOriginal<Function> {
    /// Loads the trampoline using the signature paired by
    /// [`NativeHookSpec::typed_event`].
    pub(crate) fn load(&self) -> Option<Function> {
        let pointer = self.pointer.load(Ordering::Acquire);
        if pointer.is_null() {
            return None;
        }
        assert_eq!(
            std::mem::size_of::<Function>(),
            std::mem::size_of::<*mut c_void>(),
            "native function pointer has an unsupported representation"
        );
        // SAFETY: typed_event accepts the replacement and this storage under
        // the same Function type, while Gum publishes that target's original
        // trampoline into this storage.
        Some(unsafe { std::mem::transmute_copy(&pointer) })
    }
}

/// One physical native detour. Logical events sharing an address must be
/// multiplexed by the family replacement behind a single specification.
pub(crate) struct NativeHookSpec {
    id:          &'static str,
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
            id: name,
            name,
            target,
            replacement,
            original,
        }
    }

    pub(crate) fn typed_event<Function: Copy>(
        id: &'static str,
        name: &'static str,
        target: usize,
        replacement: Function,
        original: &'static NativeOriginal<Function>,
    ) -> Self {
        assert_eq!(
            std::mem::size_of::<Function>(),
            std::mem::size_of::<usize>(),
            "native function pointer has an unsupported representation"
        );
        // SAFETY: event-family constructors pass an extern function item cast
        // to their declared ABI alias. Function and usize have equal size.
        let replacement = unsafe { std::mem::transmute_copy(&replacement) };
        Self {
            id,
            name,
            target,
            replacement,
            original: original.storage(),
        }
    }

    pub(crate) const fn id(&self) -> &'static str {
        self.id
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
