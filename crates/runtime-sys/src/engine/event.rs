use std::ffi::c_void;

use nwnrs_runtime::{EngineClassLayouts, EventContext, EventTarget, event_name};

use super::{
    abi::{CExoString, RunScript},
    address::{GlobalStorage, HookTarget, NativeAddress, Resolver},
    string::copy_exo_string,
    thread::EngineThreadToken,
};
use crate::bridge::BridgeInstallError;

pub(crate) struct EventEngine {
    load_module_finish:      NativeAddress<HookTarget>,
    virtual_machine_storage: NativeAddress<GlobalStorage>,
    run_script:              RunScript,
    recursion_level_offset:  usize,
    script_array_offset:     usize,
    script_slot_count:       usize,
    script_size:             usize,
    script_name_offset:      usize,
    script_event_id_offset:  usize,
}

impl EventEngine {
    pub(crate) fn resolve(
        resolver: &Resolver,
        target: &EventTarget,
        layouts: &EngineClassLayouts,
    ) -> Result<Self, BridgeInstallError> {
        let load_module_finish = resolver.resolve::<HookTarget>(
            "events",
            "load_module_finish",
            &target.load_module_finish,
        )?;
        let virtual_machine_storage = resolver.resolve::<GlobalStorage>(
            "events",
            "virtual_machine",
            &target.virtual_machine,
        )?;
        let run_script_address =
            resolver.resolve::<HookTarget>("events", "run_script", &target.run_script)?;
        // SAFETY: the exact target pack binds this address to
        // CVirtualMachine::RunScript for the selected server binary.
        let run_script =
            unsafe { std::mem::transmute::<usize, RunScript>(run_script_address.get()) };
        Ok(Self {
            load_module_finish,
            virtual_machine_storage,
            run_script,
            recursion_level_offset: checked(
                "vm_recursion_level_offset",
                layouts.vm_recursion_level_offset,
            )?,
            script_array_offset: checked("vm_script_array_offset", layouts.vm_script_array_offset)?,
            script_slot_count: usize::try_from(layouts.vm_script_slot_count)
                .map_err(|_error| BridgeInstallError::new("VM script-slot count exceeds usize"))?,
            script_size: checked("vm_script_size", layouts.vm_script_size)?,
            script_name_offset: checked("vm_script_name_offset", layouts.vm_script_name_offset)?,
            script_event_id_offset: checked(
                "vm_script_event_id_offset",
                layouts.vm_script_event_id_offset,
            )?,
        })
    }

    pub(crate) fn module_load_hook_target(&self) -> usize {
        self.load_module_finish.get()
    }

    pub(crate) fn run_module_onload(
        &self,
        _thread: &EngineThreadToken,
    ) -> Result<bool, BridgeInstallError> {
        let storage = self.virtual_machine_storage.get() as *const *mut c_void;
        // SAFETY: the target pack identifies live global g_pVirtualMachine
        // storage and this read is synchronous on the engine thread.
        let vm = unsafe { storage.read() };
        if vm.is_null() {
            return Err(BridgeInstallError::new(
                "g_pVirtualMachine was null at module-load completion",
            ));
        }
        let mut script_name = b"_nwnrs_onload\0".to_vec();
        let mut script = CExoString {
            string:        script_name.as_mut_ptr().cast(),
            string_length: 13,
            buffer_length: 14,
        };
        Ok((self.run_script)(vm, &raw mut script, 0, 1, 0) != 0)
    }

    pub(crate) fn context(
        &self,
        _thread: &EngineThreadToken,
        vm: *mut c_void,
    ) -> Result<EventContext, BridgeInstallError> {
        if vm.is_null() {
            return Err(BridgeInstallError::new(
                "event context received a null virtual machine",
            ));
        }
        // SAFETY: the callback VM and compiler-derived field offset remain live
        // for this synchronous read.
        let level = unsafe {
            vm.cast::<u8>()
                .add(self.recursion_level_offset)
                .cast::<i32>()
                .read()
        };
        let Ok(index) = usize::try_from(level) else {
            return Ok(EventContext::default());
        };
        if index >= self.script_slot_count {
            return Err(BridgeInstallError::new(format!(
                "virtual-machine recursion level {level} exceeds the {} script slots",
                self.script_slot_count
            )));
        }
        let offset = index
            .checked_mul(self.script_size)
            .and_then(|value| self.script_array_offset.checked_add(value))
            .ok_or_else(|| BridgeInstallError::new("virtual-machine script slot overflowed"))?;
        // SAFETY: index is bounded and target validation bounds both fields
        // within one compiler-measured CVirtualMachineScript object.
        let slot = unsafe { vm.cast::<u8>().add(offset) };
        let id = unsafe { slot.add(self.script_event_id_offset).cast::<i32>().read() };
        if id <= 0 {
            return Ok(EventContext::default());
        }
        let script = unsafe { &*slot.add(self.script_name_offset).cast::<CExoString>() };
        Ok(EventContext {
            name: event_name(id).to_string(),
            id,
            script_name: copy_exo_string(script)?,
            phase: "running".to_string(),
            depth: level
                .checked_add(1)
                .ok_or_else(|| BridgeInstallError::new("event recursion depth overflowed"))?,
        })
    }
}

fn checked(name: &str, value: u64) -> Result<usize, BridgeInstallError> {
    usize::try_from(value).map_err(|_error| {
        BridgeInstallError::new(format!("target-pack layout {name} exceeds usize"))
    })
}
