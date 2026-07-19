use std::ffi::c_void;

use nwnrs_runtime::{EngineClassLayouts, EventContext, event_name};

use super::{abi::CExoString, string::copy_exo_string, thread::EngineThreadToken};
use crate::bridge::BridgeInstallError;

pub(crate) struct EventEngine {
    recursion_level_offset: usize,
    script_array_offset:    usize,
    script_slot_count:      usize,
    script_size:            usize,
    script_name_offset:     usize,
    script_event_id_offset: usize,
}

impl EventEngine {
    pub(crate) fn from_layouts(layouts: &EngineClassLayouts) -> Result<Self, BridgeInstallError> {
        Ok(Self {
            recursion_level_offset: checked(
                "vm_recursion_level_offset",
                layouts.vm_recursion_level_offset,
            )?,
            script_array_offset:    checked(
                "vm_script_array_offset",
                layouts.vm_script_array_offset,
            )?,
            script_slot_count:      usize::try_from(layouts.vm_script_slot_count)
                .map_err(|_error| BridgeInstallError::new("VM script-slot count exceeds usize"))?,
            script_size:            checked("vm_script_size", layouts.vm_script_size)?,
            script_name_offset:     checked(
                "vm_script_name_offset",
                layouts.vm_script_name_offset,
            )?,
            script_event_id_offset: checked(
                "vm_script_event_id_offset",
                layouts.vm_script_event_id_offset,
            )?,
        })
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
