use std::{ffi::CString, marker::PhantomData, num::NonZeroUsize};

use nwnrs_runtime::TargetAddress;

use crate::bridge::BridgeInstallError;

pub(crate) enum HookTarget {}
pub(crate) enum FunctionTarget {}
pub(crate) enum GlobalStorage {}

#[derive(Clone, Copy)]
pub(crate) struct NativeFunction<Function> {
    address: usize,
    marker:  PhantomData<fn() -> Function>,
}

impl<Function: Copy> NativeFunction<Function> {
    pub(crate) fn get(self) -> Function {
        assert_eq!(
            std::mem::size_of::<Function>(),
            std::mem::size_of::<usize>(),
            "native function pointer has an unsupported representation"
        );
        // SAFETY: construction is restricted to a resolved FunctionTarget and
        // the caller selects the ABI alias declared for that target-pack key.
        unsafe { std::mem::transmute_copy(&self.address) }
    }
}

pub(crate) struct NativeAddress<T> {
    value:  NonZeroUsize,
    marker: PhantomData<fn() -> T>,
}

impl<T> NativeAddress<T> {
    pub(crate) fn get(&self) -> usize {
        self.value.get()
    }
}

impl NativeAddress<FunctionTarget> {
    pub(crate) fn function<Function: Copy>(&self) -> NativeFunction<Function> {
        NativeFunction {
            address: self.get(),
            marker:  PhantomData,
        }
    }
}

pub(crate) struct Resolver {
    module: *mut frida_gum_sys::GumModule,
}

impl Resolver {
    pub(crate) fn new(module: *mut frida_gum_sys::GumModule) -> Result<Self, BridgeInstallError> {
        if module.is_null() {
            return Err(BridgeInstallError::new(
                "native address resolver received a null main module",
            ));
        }
        Ok(Self {
            module,
        })
    }

    pub(crate) fn resolve<T>(
        &self,
        section: &str,
        name: &str,
        address: &TargetAddress,
    ) -> Result<NativeAddress<T>, BridgeInstallError> {
        let resolved = match address {
            TargetAddress::Symbol {
                symbol,
            } => {
                let symbol = CString::new(symbol.as_str()).map_err(|_error| {
                    BridgeInstallError::new(format!(
                        "target pack {section}.{name} contains a NUL byte"
                    ))
                })?;
                // SAFETY: the resolver retains a valid module for this pass and
                // CString guarantees a terminated symbol name.
                unsafe {
                    let local =
                        frida_gum_sys::gum_module_find_symbol_by_name(self.module, symbol.as_ptr());
                    if local == 0 {
                        frida_gum_sys::gum_module_find_global_export_by_name(symbol.as_ptr())
                    } else {
                        local
                    }
                }
            }
            TargetAddress::Offset {
                offset,
            } => {
                // SAFETY: the resolver owns a live main-module reference.
                let range = unsafe { frida_gum_sys::gum_module_get_range(self.module) };
                if range.is_null() {
                    return Err(BridgeInstallError::new(
                        "Frida Gum returned no range for the main executable",
                    ));
                }
                // SAFETY: range is borrowed from the retained module.
                let range = unsafe { &*range };
                if *offset >= range.size {
                    return Err(BridgeInstallError::new(format!(
                        "target pack {section}.{name} offset {offset:#x} is outside the main \
                         executable"
                    )));
                }
                range.base_address.checked_add(*offset).ok_or_else(|| {
                    BridgeInstallError::new(format!(
                        "target pack {section}.{name} address overflowed"
                    ))
                })?
            }
        };
        let resolved = usize::try_from(resolved).map_err(|_error| {
            BridgeInstallError::new(format!(
                "target pack {section}.{name} address exceeds usize"
            ))
        })?;
        let value = NonZeroUsize::new(resolved).ok_or_else(|| {
            BridgeInstallError::new(format!(
                "could not resolve target pack {section}.{name} in the main executable"
            ))
        })?;
        Ok(NativeAddress {
            value,
            marker: PhantomData,
        })
    }
}
