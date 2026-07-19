use std::{ffi::c_void, mem};

use nwnrs_runtime::{BridgeTarget, EngineClassLayouts, Vector};

use super::{
    abi::{
        CExoString, EngineVector, FreeExoStringBuffer, FunctionManagement, ObjectId, StackPopFloat,
        StackPopInteger, StackPopObject, StackPopString, StackPopVector, StackPushFloat,
        StackPushInteger, StackPushObject, StackPushString, StackPushVector,
    },
    address::{HookTarget, NativeAddress, Resolver},
    string::OwnedEngineString,
    thread::EngineThreadToken,
};
use crate::{bridge::BridgeInstallError, write_diagnostic};

const VM_STACK_OVERFLOW: i32 = -638;
const VM_STACK_UNDERFLOW: i32 = -639;
const VM_FAKE_ABORT_SCRIPT: i32 = -645;

pub(crate) struct VirtualMachineEngine {
    command_implementer_vm_offset: usize,
    function_management:           NativeAddress<HookTarget>,
    stack_pop_integer:             StackPopInteger,
    stack_push_integer:            StackPushInteger,
    stack_pop_float:               StackPopFloat,
    stack_push_float:              StackPushFloat,
    stack_pop_object:              StackPopObject,
    stack_push_object:             StackPushObject,
    stack_pop_string:              StackPopString,
    stack_push_string:             StackPushString,
    stack_pop_vector:              StackPopVector,
    stack_push_vector:             StackPushVector,
    free_exo_string_buffer:        FreeExoStringBuffer,
}

impl VirtualMachineEngine {
    pub(crate) fn resolve(
        resolver: &Resolver,
        target: &BridgeTarget,
        layouts: &EngineClassLayouts,
    ) -> Result<Self, BridgeInstallError> {
        let command_implementer_vm_offset = checked_offset(
            "layouts.classes.command_implementer_vm_offset",
            layouts.command_implementer_vm_offset,
        )?;
        macro_rules! resolve_function {
            ($name:literal, $field:ident, $ty:ty) => {{
                let address = resolver.resolve::<$ty>("bridge", $name, &target.$field)?;
                // SAFETY: target-pack validation binds this address to the named
                // Unified function signature for this exact executable hash.
                unsafe { mem::transmute::<usize, $ty>(address.get()) }
            }};
        }
        Ok(Self {
            command_implementer_vm_offset,
            function_management: resolver.resolve::<HookTarget>(
                "bridge",
                "function_management",
                &target.function_management,
            )?,
            stack_pop_integer: resolve_function!(
                "stack_pop_integer",
                stack_pop_integer,
                StackPopInteger
            ),
            stack_push_integer: resolve_function!(
                "stack_push_integer",
                stack_push_integer,
                StackPushInteger
            ),
            stack_pop_float: resolve_function!("stack_pop_float", stack_pop_float, StackPopFloat),
            stack_push_float: resolve_function!(
                "stack_push_float",
                stack_push_float,
                StackPushFloat
            ),
            stack_pop_object: resolve_function!(
                "stack_pop_object",
                stack_pop_object,
                StackPopObject
            ),
            stack_push_object: resolve_function!(
                "stack_push_object",
                stack_push_object,
                StackPushObject
            ),
            stack_pop_string: resolve_function!(
                "stack_pop_string",
                stack_pop_string,
                StackPopString
            ),
            stack_push_string: resolve_function!(
                "stack_push_string",
                stack_push_string,
                StackPushString
            ),
            stack_pop_vector: resolve_function!(
                "stack_pop_vector",
                stack_pop_vector,
                StackPopVector
            ),
            stack_push_vector: resolve_function!(
                "stack_push_vector",
                stack_push_vector,
                StackPushVector
            ),
            free_exo_string_buffer: resolve_function!(
                "free_exo_string_buffer",
                free_exo_string_buffer,
                FreeExoStringBuffer
            ),
        })
    }

    pub(crate) fn hook_target(&self) -> usize {
        self.function_management.get()
    }

    pub(crate) fn virtual_machine(
        &self,
        _thread: &EngineThreadToken,
        commands: *mut c_void,
    ) -> Result<*mut c_void, BridgeInstallError> {
        if commands.is_null() {
            return Err(BridgeInstallError::new(
                "engine passed a null command implementer",
            ));
        }
        // SAFETY: the callback supplies a live command implementer and the
        // compiler-verified target layout identifies its aligned VM field.
        let vm = unsafe {
            commands
                .cast::<u8>()
                .add(self.command_implementer_vm_offset)
                .cast::<*mut c_void>()
                .read()
        };
        if vm.is_null() {
            return Err(BridgeInstallError::new(
                "command implementer contains a null virtual-machine pointer",
            ));
        }
        Ok(vm)
    }

    pub(crate) fn pop_integer(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
    ) -> Result<i32, i32> {
        let mut value = 0;
        bool_result(
            (self.stack_pop_integer)(vm, &raw mut value),
            VM_STACK_UNDERFLOW,
        )?;
        Ok(value)
    }

    pub(crate) fn push_integer(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
        value: i32,
    ) -> Result<(), i32> {
        bool_result((self.stack_push_integer)(vm, value), VM_STACK_OVERFLOW)
    }

    pub(crate) fn pop_float(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
    ) -> Result<f32, i32> {
        let mut value = 0.0;
        bool_result(
            (self.stack_pop_float)(vm, &raw mut value),
            VM_STACK_UNDERFLOW,
        )?;
        Ok(value)
    }

    pub(crate) fn push_float(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
        value: f32,
    ) -> Result<(), i32> {
        bool_result((self.stack_push_float)(vm, value), VM_STACK_OVERFLOW)
    }

    pub(crate) fn pop_object(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
    ) -> Result<ObjectId, i32> {
        let mut value = 0;
        bool_result(
            (self.stack_pop_object)(vm, &raw mut value),
            VM_STACK_UNDERFLOW,
        )?;
        Ok(ObjectId::from_raw(value))
    }

    pub(crate) fn push_object(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
        value: ObjectId,
    ) -> Result<(), i32> {
        bool_result((self.stack_push_object)(vm, value.raw()), VM_STACK_OVERFLOW)
    }

    pub(crate) fn pop_string(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
    ) -> Result<Vec<u8>, i32> {
        let mut value = OwnedEngineString::empty(self.free_exo_string_buffer);
        bool_result(
            (self.stack_pop_string)(vm, value.as_mut_ptr()),
            VM_STACK_UNDERFLOW,
        )?;
        value.copy().map_err(|error| {
            write_diagnostic(&error.to_string());
            VM_FAKE_ABORT_SCRIPT
        })
    }

    pub(crate) fn push_string(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
        value: &[u8],
    ) -> Result<(), i32> {
        let string_length = u32::try_from(value.len()).map_err(|_error| VM_FAKE_ABORT_SCRIPT)?;
        let buffer_length = string_length.checked_add(1).ok_or(VM_FAKE_ABORT_SCRIPT)?;
        let mut bytes = Vec::with_capacity(value.len().saturating_add(1));
        bytes.extend_from_slice(value);
        bytes.push(0);
        let string = CExoString {
            string: bytes.as_mut_ptr().cast(),
            string_length,
            buffer_length,
        };
        bool_result(
            (self.stack_push_string)(vm, &raw const string),
            VM_STACK_OVERFLOW,
        )
    }

    pub(crate) fn pop_vector(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
    ) -> Result<Vector, i32> {
        let mut value = EngineVector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        bool_result(
            (self.stack_pop_vector)(vm, &raw mut value),
            VM_STACK_UNDERFLOW,
        )?;
        Ok(Vector {
            x: value.x,
            y: value.y,
            z: value.z,
        })
    }

    pub(crate) fn push_vector(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
        value: Vector,
    ) -> Result<(), i32> {
        bool_result(
            (self.stack_push_vector)(
                vm,
                EngineVector {
                    x: value.x,
                    y: value.y,
                    z: value.z,
                },
            ),
            VM_STACK_OVERFLOW,
        )
    }
}

fn checked_offset(name: &str, value: u64) -> Result<usize, BridgeInstallError> {
    usize::try_from(value).map_err(|_error| {
        BridgeInstallError::new(format!("target-pack offset {name} exceeds usize"))
    })
}

fn bool_result(value: i32, error: i32) -> Result<(), i32> {
    if value == 0 { Err(error) } else { Ok(()) }
}

const _: FunctionManagement = crate::bridge::function_management_replacement;
