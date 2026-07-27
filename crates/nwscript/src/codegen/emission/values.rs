use super::super::*;

pub(in crate::codegen) fn emit_push_literal(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    literal: &Literal,
    ty: &SemanticType,
    span: Option<crate::Span>,
) -> Result<(), CodegenError> {
    match literal {
        Literal::Integer(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   value.to_be_bytes().to_vec(),
        }),
        Literal::Float(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeFloat,
            extra:   value.to_bits().to_be_bytes().to_vec(),
        }),
        Literal::String(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeString,
            extra:   string_extra(value)?,
        }),
        Literal::ObjectSelf => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeObject,
            extra:   0_i32.to_be_bytes().to_vec(),
        }),
        Literal::ObjectInvalid => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeObject,
            extra:   1_i32.to_be_bytes().to_vec(),
        }),
        Literal::LocationInvalid => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeEngst2,
            extra:   0_u32.to_be_bytes().to_vec(),
        }),
        Literal::Json(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeEngst7,
            extra:   string_extra_bytes(value.as_bytes())?,
        }),
        Literal::Vector(values) => {
            for value in values {
                compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::Constant,
                    auxcode: NcsAuxCode::TypeFloat,
                    extra:   value.to_bits().to_be_bytes().to_vec(),
                });
            }
        }
        Literal::Magic(magic) => {
            let resolved = compiler.magic_literal_value(*magic, span)?;
            return emit_push_literal(compiler, temp_bytes, &resolved, ty, span);
        }
    }

    *temp_bytes += size_of_type(ty, &compiler.structs)?;
    Ok(())
}

pub(in crate::codegen) fn format_magic_date(timestamp: SystemTime) -> String {
    let seconds = timestamp
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let days = i64::try_from(seconds / 86_400).ok().unwrap_or(i64::MAX);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

pub(in crate::codegen) fn format_magic_time(timestamp: SystemTime) -> String {
    let seconds = timestamp
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{hour:02}:{minute:02}:{second:02}")
}

pub(in crate::codegen) fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(m <= 2);
    (
        i32::try_from(year).ok().unwrap_or(i32::MAX),
        u32::try_from(m).ok().unwrap_or(1),
        u32::try_from(d).ok().unwrap_or(1),
    )
}

pub(in crate::codegen) fn assignment_extra(
    offset: i32,
    size: usize,
) -> Result<Vec<u8>, CodegenError> {
    let mut bytes = Vec::with_capacity(6);
    bytes.extend_from_slice(&offset.to_be_bytes());
    let size = usize_to_u16(size, "assignment size")?;
    bytes.extend_from_slice(&size.to_be_bytes());
    Ok(bytes)
}

pub(in crate::codegen) fn builtin_call_extra(id: u16, argc: u8) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(3);
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.push(argc);
    bytes
}

pub(in crate::codegen) fn store_state_extra(global_size: u32, stack_size: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&global_size.to_be_bytes());
    bytes.extend_from_slice(&stack_size.to_be_bytes());
    bytes
}

pub(in crate::codegen) fn emit_copy_top_bytes(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    size: usize,
) -> Result<(), CodegenError> {
    compiler.assembler.push(NcsInstruction {
        opcode:  NcsOpcode::RunstackCopy,
        auxcode: NcsAuxCode::TypeVoid,
        extra:   assignment_extra(-usize_to_i32(size, "copy size")?, size)?,
    });
    *temp_bytes += size;
    Ok(())
}

pub(in crate::codegen) fn enum_backing_semantic_type(
    backing: crate::EnumBackingType,
) -> SemanticType {
    match backing {
        crate::EnumBackingType::Int => SemanticType::Int,
        crate::EnumBackingType::String => SemanticType::String,
    }
}

pub(in crate::codegen) fn engine_structure_index(
    name: &str,
    langspec: Option<&LangSpec>,
) -> Result<u8, CodegenError> {
    if let Some(langspec) = langspec
        && let Some(index) = langspec
            .engine_structures
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        return u8::try_from(index).map_err(|_error| {
            CodegenError::new(
                None,
                format!("engine structure index out of range for {name:?}"),
            )
        });
    }

    let fallback = [
        "effect",
        "event",
        "location",
        "talent",
        "itemproperty",
        "sqlquery",
        "cassowary",
        "json",
        "vector",
    ];
    fallback
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .and_then(|index| u8::try_from(index).ok())
        .ok_or_else(|| CodegenError::new(None, format!("unknown engine structure {name:?}")))
}

pub(in crate::codegen) fn simple_instruction(opcode: NcsOpcode) -> NcsInstruction {
    NcsInstruction {
        opcode,
        auxcode: NcsAuxCode::None,
        extra: Vec::new(),
    }
}

pub(in crate::codegen) fn simple_aux_instruction(
    opcode: NcsOpcode,
    auxcode: NcsAuxCode,
) -> NcsInstruction {
    NcsInstruction {
        opcode,
        auxcode,
        extra: Vec::new(),
    }
}
