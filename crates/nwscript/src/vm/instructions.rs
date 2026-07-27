use super::*;

pub(super) fn read_u8(decoded: &VmProgramInstruction, start: usize) -> Result<u8, VmError> {
    decoded
        .instruction
        .extra
        .get(start)
        .copied()
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))
}

pub(super) fn read_u16(decoded: &VmProgramInstruction, start: usize) -> Result<u16, VmError> {
    let window = decoded
        .instruction
        .extra
        .get(start..start + 2)
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))?;
    let bytes =
        <[u8; 2]>::try_from(window).map_err(|_error| invalid_extra(decoded, "bad u16 payload"))?;
    Ok(u16::from_be_bytes(bytes))
}

pub(super) fn read_i32(decoded: &VmProgramInstruction, start: usize) -> Result<i32, VmError> {
    let window = decoded
        .instruction
        .extra
        .get(start..start + 4)
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))?;
    let bytes =
        <[u8; 4]>::try_from(window).map_err(|_error| invalid_extra(decoded, "bad i32 payload"))?;
    Ok(i32::from_be_bytes(bytes))
}

pub(super) fn read_u32(decoded: &VmProgramInstruction, start: usize) -> Result<u32, VmError> {
    let window = decoded
        .instruction
        .extra
        .get(start..start + 4)
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))?;
    let bytes =
        <[u8; 4]>::try_from(window).map_err(|_error| invalid_extra(decoded, "bad u32 payload"))?;
    Ok(u32::from_be_bytes(bytes))
}

pub(super) fn read_f32(decoded: &VmProgramInstruction, start: usize) -> Result<f32, VmError> {
    Ok(f32::from_bits(read_u32(decoded, start)?))
}

pub(super) fn read_ncs_string(decoded: &VmProgramInstruction) -> Result<ScriptString, VmError> {
    let length = usize::from(read_u16(decoded, 0)?);
    let window = decoded
        .instruction
        .extra
        .get(2..2 + length)
        .ok_or_else(|| invalid_extra(decoded, "string payload shorter than declared length"))?;
    Ok(ScriptString::new(window.to_vec()))
}

pub(super) fn read_ncs_text(decoded: &VmProgramInstruction) -> Result<String, VmError> {
    Ok(read_ncs_string(decoded)?.display_text())
}

pub(super) fn relative_stack_cell(
    decoded: &VmProgramInstruction,
    base: usize,
    encoded_offset: i32,
) -> Result<usize, VmError> {
    if encoded_offset > 0 || encoded_offset % 4 != 0 {
        return Err(invalid_extra(
            decoded,
            &format!("expected negative 4-byte-aligned stack offset, got {encoded_offset}"),
        ));
    }
    let cells = usize::try_from(encoded_offset.unsigned_abs() / 4)
        .map_err(|_error| invalid_extra(decoded, "invalid negative stack offset"))?;
    base.checked_sub(cells)
        .ok_or_else(|| VmError::StackUnderflow {
            message: format!("stack offset {encoded_offset} underflowed base {base}"),
        })
}

pub(super) fn jump_target(current: usize, delta: i32) -> Result<usize, VmError> {
    if delta >= 0 {
        current
            .checked_add(usize::try_from(delta).ok().unwrap_or(usize::MAX))
            .ok_or(VmError::InvalidInstructionPointer {
                offset: current
            })
    } else {
        current
            .checked_sub(
                usize::try_from(delta.unsigned_abs())
                    .ok()
                    .unwrap_or(usize::MAX),
            )
            .ok_or(VmError::InvalidInstructionPointer {
                offset: current
            })
    }
}

pub(super) fn capture_saved_situation(
    script: &VmScript,
    decoded: &VmProgramInstruction,
    target_ip: usize,
) -> Result<VmSituation, VmError> {
    let global_bytes = usize::try_from(read_u32(decoded, 0)?)
        .map_err(|_error| invalid_extra(decoded, "global size exceeds usize range"))?;
    let stack_bytes = usize::try_from(read_u32(decoded, 4)?)
        .map_err(|_error| invalid_extra(decoded, "stack size exceeds usize range"))?;
    if global_bytes % 4 != 0 || stack_bytes % 4 != 0 {
        return Err(invalid_extra(
            decoded,
            "saved state sizes must be 4-byte aligned",
        ));
    }

    let saved_sp = (global_bytes + stack_bytes) / 4;
    if saved_sp > script.sp {
        return Err(VmError::StackUnderflow {
            message: format!(
                "saved situation requested {} cells, but stack only has {}",
                saved_sp, script.sp
            ),
        });
    }

    Ok(VmSituation {
        label:   script.label.clone(),
        program: script.program.clone(),
        ip:      target_ip,
        sp:      saved_sp,
        bp:      script.bp,
        stack:   script
            .stack
            .get(..saved_sp)
            .ok_or_else(|| VmError::StackUnderflow {
                message: format!(
                    "saved situation requested {} cells, but stack only has {}",
                    saved_sp,
                    script.stack.len()
                ),
            })?
            .to_vec(),
    })
}

pub(super) fn default_engine_structure_value(index: u8) -> VmEngineStructureValue {
    if index == 7 {
        VmEngineStructureValue::Text(String::new())
    } else {
        VmEngineStructureValue::Word(0)
    }
}

pub(super) fn consume_abort_request(script: &mut VmScript) -> bool {
    if script.abort_requested {
        script.abort_requested = false;
        script.aborted = true;
        script.ret.clear();
        return true;
    }
    false
}

pub(super) fn cells_for_ndb_type(ndb: &Ndb, ty: &NdbType) -> Result<usize, VmError> {
    Ok(match ty {
        NdbType::Float
        | NdbType::Int
        | NdbType::Object
        | NdbType::String
        | NdbType::EngineStructure(_) => 1,
        NdbType::Void => 0,
        NdbType::Struct(struct_index) => ndb
            .structs
            .get(*struct_index)
            .ok_or_else(|| VmError::Setup {
                message: format!("NDB references missing structure t{struct_index:04}"),
            })?
            .fields
            .iter()
            .try_fold(0usize, |total, field| {
                cells_for_ndb_type(ndb, &field.ty).map(|width| total + width)
            })?,
        NdbType::Unknown | NdbType::Raw(_) => {
            return Err(VmError::Setup {
                message: format!("unsupported VM function metadata type {ty}"),
            });
        }
    })
}

pub(super) fn check_run_limits(script: &VmScript, options: VmRunOptions) -> Result<(), VmError> {
    if let Some(limit) = options.max_recursion_depth
        && script.ret.len() > limit
    {
        return Err(VmError::RecursionLimitExceeded {
            depth: script.ret.len(),
            limit,
        });
    }
    if let Some(limit) = options.max_stack_cells
        && script.stack.len() > limit
    {
        return Err(VmError::StackLimitExceeded {
            cells: script.stack.len(),
            limit,
        });
    }
    Ok(())
}

pub(super) fn push_default_value(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
    vm: &Vm,
) -> Result<(), VmError> {
    match decoded.instruction.auxcode {
        NcsAuxCode::TypeInteger => script.push_int(0),
        NcsAuxCode::TypeFloat => script.push_float(0.0),
        NcsAuxCode::TypeString => script.push_string(ScriptString::default()),
        NcsAuxCode::TypeObject => script.push_object(0),
        aux => {
            let Some(index) = engine_structure_index(aux) else {
                return unsupported(decoded, "RSADD does not support this auxcode");
            };
            script.push_engine_structure(index, vm.default_engine_structure_value(index));
        }
    }
    Ok(())
}

pub(super) fn push_constant_value(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
) -> Result<(), VmError> {
    match decoded.instruction.auxcode {
        NcsAuxCode::TypeInteger => script.push_int(read_i32(decoded, 0)?),
        NcsAuxCode::TypeFloat => script.push_float(read_f32(decoded, 0)?),
        NcsAuxCode::TypeString => script.push_string(read_ncs_string(decoded)?),
        NcsAuxCode::TypeObject => script.push_object(read_u32(decoded, 0)?),
        aux => {
            let Some(index) = engine_structure_index(aux) else {
                return unsupported(decoded, "CONST does not support this auxcode");
            };
            let value = if index == 7 {
                VmEngineStructureValue::Text(read_ncs_text(decoded)?)
            } else {
                VmEngineStructureValue::Word(read_u32(decoded, 0)?)
            };
            script.push(VmValue::EngineStructure {
                index,
                value,
            });
        }
    }
    Ok(())
}

pub(super) fn engine_structure_index(auxcode: NcsAuxCode) -> Option<u8> {
    match auxcode {
        NcsAuxCode::TypeEngst0 => Some(0),
        NcsAuxCode::TypeEngst1 => Some(1),
        NcsAuxCode::TypeEngst2 => Some(2),
        NcsAuxCode::TypeEngst3 => Some(3),
        NcsAuxCode::TypeEngst4 => Some(4),
        NcsAuxCode::TypeEngst5 => Some(5),
        NcsAuxCode::TypeEngst6 => Some(6),
        NcsAuxCode::TypeEngst7 => Some(7),
        NcsAuxCode::TypeEngst8 => Some(8),
        NcsAuxCode::TypeEngst9 => Some(9),
        _ => None,
    }
}

pub(super) fn apply_comparison(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
    vm: &Vm,
) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorVector {
        let rhs = script.pop_vector()?;
        let lhs = script.pop_vector()?;
        let result = match decoded.instruction.opcode {
            NcsOpcode::Equal => lhs == rhs,
            NcsOpcode::NotEqual => lhs != rhs,
            _ => return unsupported(decoded, "ordered comparisons are not valid for vectors"),
        };
        script.push_int(bool_to_int(result));
        return Ok(());
    }

    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeStructStruct {
        let size_bytes = usize::from(read_u16(decoded, 0)?);
        let cell_count = size_bytes / 4;
        let rhs_start =
            script
                .sp
                .checked_sub(cell_count)
                .ok_or_else(|| VmError::StackUnderflow {
                    message: format!("missing {} cells for rhs struct comparison", cell_count),
                })?;
        let lhs_start =
            rhs_start
                .checked_sub(cell_count)
                .ok_or_else(|| VmError::StackUnderflow {
                    message: format!("missing {} cells for lhs struct comparison", cell_count),
                })?;
        let equal =
            script.stack.get(lhs_start..rhs_start) == script.stack.get(rhs_start..script.sp);
        script.set_stack_pointer(lhs_start)?;
        let result = match decoded.instruction.opcode {
            NcsOpcode::Equal => equal,
            NcsOpcode::NotEqual => !equal,
            _ => return unsupported(decoded, "ordered comparisons are not valid for structs"),
        };
        script.push_int(bool_to_int(result));
        return Ok(());
    }

    let rhs = script.pop()?;
    let lhs = script.pop()?;
    let result = match (&lhs, &rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, rhs)
        }
        (VmValue::Int(lhs), VmValue::Float(rhs)) => {
            compare_ordered(decoded.instruction.opcode, &(*lhs as f32), rhs)
        }
        (VmValue::Float(lhs), VmValue::Int(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, &(*rhs as f32))
        }
        (VmValue::Float(lhs), VmValue::Float(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, rhs)
        }
        (VmValue::String(lhs), VmValue::String(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, rhs)
        }
        (VmValue::Object(lhs), VmValue::Object(rhs)) => compare_equality(decoded, lhs, rhs)?,
        (
            VmValue::EngineStructure {
                index: lhs_index,
                value: lhs_value,
            },
            VmValue::EngineStructure {
                index: rhs_index,
                value: rhs_value,
            },
        ) if lhs_index == rhs_index => {
            compare_engine_structure_equality(decoded, vm, *lhs_index, lhs_value, rhs_value)?
        }
        _ => {
            return unsupported(
                decoded,
                &format!(
                    "comparison between {} and {} is not implemented",
                    lhs.kind_name(),
                    rhs.kind_name()
                ),
            );
        }
    };
    script.push_int(bool_to_int(result));
    Ok(())
}

pub(super) fn apply_add(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorVector {
        let rhs = script.pop_vector()?;
        let lhs = script.pop_vector()?;
        script.push_vector([lhs[0] + rhs[0], lhs[1] + rhs[1], lhs[2] + rhs[2]]);
        return Ok(());
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => script.push_int(lhs.wrapping_add(rhs)),
        (VmValue::Int(lhs), VmValue::Float(rhs)) => script.push_float(lhs as f32 + rhs),
        (VmValue::Float(lhs), VmValue::Int(rhs)) => script.push_float(lhs + rhs as f32),
        (VmValue::Float(lhs), VmValue::Float(rhs)) => script.push_float(lhs + rhs),
        (VmValue::String(lhs), VmValue::String(rhs)) => {
            script.push_string(lhs.concat(&rhs));
        }
        _ => {
            return unsupported(
                decoded,
                "ADD currently supports int, float, string, and vector",
            );
        }
    }
    Ok(())
}

pub(super) fn apply_sub(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorVector {
        let rhs = script.pop_vector()?;
        let lhs = script.pop_vector()?;
        script.push_vector([lhs[0] - rhs[0], lhs[1] - rhs[1], lhs[2] - rhs[2]]);
        return Ok(());
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => script.push_int(lhs.wrapping_sub(rhs)),
        (VmValue::Int(lhs), VmValue::Float(rhs)) => script.push_float(lhs as f32 - rhs),
        (VmValue::Float(lhs), VmValue::Int(rhs)) => script.push_float(lhs - rhs as f32),
        (VmValue::Float(lhs), VmValue::Float(rhs)) => script.push_float(lhs - rhs),
        _ => return unsupported(decoded, "SUB currently supports int, float, and vector"),
    }
    Ok(())
}

pub(super) fn apply_mul(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
) -> Result<(), VmError> {
    match decoded.instruction.auxcode {
        NcsAuxCode::TypeTypeVectorFloat => {
            let rhs = script.pop_float()?;
            let lhs = script.pop_vector()?;
            script.push_vector([lhs[0] * rhs, lhs[1] * rhs, lhs[2] * rhs]);
            return Ok(());
        }
        NcsAuxCode::TypeTypeFloatVector => {
            let rhs = script.pop_vector()?;
            let lhs = script.pop_float()?;
            script.push_vector([lhs * rhs[0], lhs * rhs[1], lhs * rhs[2]]);
            return Ok(());
        }
        _ => {}
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => script.push_int(lhs.wrapping_mul(rhs)),
        (VmValue::Int(lhs), VmValue::Float(rhs)) => script.push_float(lhs as f32 * rhs),
        (VmValue::Float(lhs), VmValue::Int(rhs)) => script.push_float(lhs * rhs as f32),
        (VmValue::Float(lhs), VmValue::Float(rhs)) => script.push_float(lhs * rhs),
        _ => return unsupported(decoded, "MUL currently supports int, float, and vector"),
    }
    Ok(())
}

pub(super) fn apply_div(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorFloat {
        let rhs = script.pop_float()?;
        if rhs == 0.0 {
            return Err(VmError::DivideByZero {
                offset: decoded.offset,
            });
        }
        let lhs = script.pop_vector()?;
        script.push_vector([lhs[0] / rhs, lhs[1] / rhs, lhs[2] / rhs]);
        return Ok(());
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => {
            if rhs == 0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_int(lhs.wrapping_div(rhs));
        }
        (VmValue::Int(lhs), VmValue::Float(rhs)) => {
            if rhs == 0.0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_float(lhs as f32 / rhs);
        }
        (VmValue::Float(lhs), VmValue::Int(rhs)) => {
            if rhs == 0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_float(lhs / rhs as f32);
        }
        (VmValue::Float(lhs), VmValue::Float(rhs)) => {
            if rhs == 0.0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_float(lhs / rhs);
        }
        _ => return unsupported(decoded, "DIV currently supports int, float, and vector"),
    }
    Ok(())
}

pub(super) fn compare_engine_structure_equality(
    decoded: &VmProgramInstruction,
    vm: &Vm,
    index: u8,
    lhs: &VmEngineStructureValue,
    rhs: &VmEngineStructureValue,
) -> Result<bool, VmError> {
    let equal = vm.compare_engine_structure_values(index, lhs, rhs);
    match decoded.instruction.opcode {
        NcsOpcode::Equal => Ok(equal),
        NcsOpcode::NotEqual => Ok(!equal),
        _ => unsupported(
            decoded,
            "ordered comparison is not valid for engine structures",
        ),
    }
}

pub(super) fn compare_equality<T: PartialEq>(
    decoded: &VmProgramInstruction,
    lhs: &T,
    rhs: &T,
) -> Result<bool, VmError> {
    match decoded.instruction.opcode {
        NcsOpcode::Equal => Ok(lhs == rhs),
        NcsOpcode::NotEqual => Ok(lhs != rhs),
        _ => unsupported(
            decoded,
            "ordered comparison is not valid for this runtime type",
        ),
    }
}

pub(super) fn compare_ordered<T: PartialEq + PartialOrd>(
    opcode: NcsOpcode,
    lhs: &T,
    rhs: &T,
) -> bool {
    match opcode {
        NcsOpcode::Equal => lhs == rhs,
        NcsOpcode::NotEqual => lhs != rhs,
        NcsOpcode::Lt => lhs < rhs,
        NcsOpcode::Gt => lhs > rhs,
        NcsOpcode::Leq => lhs <= rhs,
        NcsOpcode::Geq => lhs >= rhs,
        _ => false,
    }
}

pub(super) fn bool_to_int(value: bool) -> i32 {
    if value { 1 } else { 0 }
}

pub(super) fn invalid_extra(decoded: &VmProgramInstruction, message: &str) -> VmError {
    VmError::InvalidExtra {
        offset:  decoded.offset,
        opcode:  decoded.instruction.opcode,
        auxcode: decoded.instruction.auxcode,
        message: message.to_string(),
    }
}

pub(super) fn unsupported<T>(decoded: &VmProgramInstruction, message: &str) -> Result<T, VmError> {
    Err(VmError::Unsupported {
        offset:  decoded.offset,
        opcode:  decoded.instruction.opcode,
        auxcode: decoded.instruction.auxcode,
        message: message.to_string(),
    })
}
