use super::*;

pub(super) fn literal_from_builtin_value(value: &BuiltinValue) -> Option<Literal> {
    match value {
        BuiltinValue::Int(value) | BuiltinValue::ObjectId(value) => Some(Literal::Integer(*value)),
        BuiltinValue::Float(value) => Some(Literal::Float(*value)),
        BuiltinValue::String(value) => Some(Literal::String(value.clone())),
        BuiltinValue::ObjectSelf => Some(Literal::ObjectSelf),
        BuiltinValue::ObjectInvalid => Some(Literal::ObjectInvalid),
        BuiltinValue::LocationInvalid => Some(Literal::LocationInvalid),
        BuiltinValue::Json(value) => Some(Literal::Json(value.clone())),
        BuiltinValue::Vector(value) => Some(Literal::Vector(*value)),
        BuiltinValue::Raw(_) => None,
    }
}

pub(super) fn semantic_type_from_builtin_type(ty: &BuiltinType) -> SemanticType {
    match ty {
        BuiltinType::Int => SemanticType::Int,
        BuiltinType::Float => SemanticType::Float,
        BuiltinType::String => SemanticType::String,
        BuiltinType::Object => SemanticType::Object,
        BuiltinType::Void => SemanticType::Void,
        BuiltinType::Action => SemanticType::Action,
        BuiltinType::Vector => SemanticType::Vector,
        BuiltinType::EngineStructure(name) => SemanticType::EngineStructure(name.clone()),
    }
}

pub(super) fn evaluate_case_literal(
    expr: &HirExpr,
    constant_env: &BTreeMap<String, ConstValue>,
) -> Result<Literal, CodegenError> {
    match evaluate_const_expr(expr, constant_env) {
        Some(ConstValue::Int(value)) => Ok(Literal::Integer(value)),
        Some(ConstValue::String(value)) => Ok(Literal::String(value)),
        Some(ConstValue::Float(_)) | None => Err(CodegenError::new(
            Some(expr.span),
            "switch case code generation requires a constant int or string",
        )),
    }
}

pub(super) fn function_frame_bytes(layout: &FunctionLayout) -> usize {
    layout
        .return_layout
        .as_ref()
        .map_or(0, |layout| layout.size)
        + layout.parameter_size
        + layout.active_locals_size
}

pub(super) fn size_of_binary_result(
    op: BinaryOp,
    left: &SemanticType,
    right: &SemanticType,
    structs: &BTreeMap<String, &crate::HirStruct>,
) -> Result<usize, CodegenError> {
    let ty = match op {
        BinaryOp::EqualEqual
        | BinaryOp::NotEqual
        | BinaryOp::GreaterEqual
        | BinaryOp::GreaterThan
        | BinaryOp::LessThan
        | BinaryOp::LessEqual
        | BinaryOp::LogicalAnd
        | BinaryOp::LogicalOr
        | BinaryOp::InclusiveOr
        | BinaryOp::ExclusiveOr
        | BinaryOp::BooleanAnd
        | BinaryOp::ShiftLeft
        | BinaryOp::ShiftRight
        | BinaryOp::UnsignedShiftRight
        | BinaryOp::Modulus => SemanticType::Int,
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
            if left == &SemanticType::Float || right == &SemanticType::Float {
                if left == &SemanticType::Vector || right == &SemanticType::Vector {
                    SemanticType::Vector
                } else {
                    SemanticType::Float
                }
            } else if left == &SemanticType::String {
                SemanticType::String
            } else if left == &SemanticType::Vector {
                SemanticType::Vector
            } else {
                left.clone()
            }
        }
    };
    size_of_type(&ty, structs)
}

pub(super) fn opcode_for_binary(op: BinaryOp) -> NcsOpcode {
    match op {
        BinaryOp::Multiply => NcsOpcode::Mul,
        BinaryOp::Divide => NcsOpcode::Div,
        BinaryOp::Modulus => NcsOpcode::Modulus,
        BinaryOp::Add => NcsOpcode::Add,
        BinaryOp::Subtract => NcsOpcode::Sub,
        BinaryOp::ShiftLeft => NcsOpcode::ShiftLeft,
        BinaryOp::ShiftRight => NcsOpcode::ShiftRight,
        BinaryOp::UnsignedShiftRight => NcsOpcode::UShiftRight,
        BinaryOp::GreaterEqual => NcsOpcode::Geq,
        BinaryOp::GreaterThan => NcsOpcode::Gt,
        BinaryOp::LessThan => NcsOpcode::Lt,
        BinaryOp::LessEqual => NcsOpcode::Leq,
        BinaryOp::NotEqual => NcsOpcode::NotEqual,
        BinaryOp::EqualEqual => NcsOpcode::Equal,
        BinaryOp::BooleanAnd => NcsOpcode::BooleanAnd,
        BinaryOp::ExclusiveOr => NcsOpcode::ExclusiveOr,
        BinaryOp::InclusiveOr => NcsOpcode::InclusiveOr,
        BinaryOp::LogicalAnd => NcsOpcode::LogicalAnd,
        BinaryOp::LogicalOr => NcsOpcode::LogicalOr,
    }
}

pub(super) fn aux_for_binary(
    left: &SemanticType,
    right: &SemanticType,
    langspec: Option<&LangSpec>,
) -> Result<NcsAuxCode, CodegenError> {
    if let SemanticType::Enum {
        backing, ..
    } = left
    {
        return aux_for_binary(&enum_backing_semantic_type(*backing), right, langspec);
    }
    if let SemanticType::Enum {
        backing, ..
    } = right
    {
        return aux_for_binary(left, &enum_backing_semantic_type(*backing), langspec);
    }
    match (left, right) {
        (SemanticType::Int, SemanticType::Int) => Ok(NcsAuxCode::TypeTypeIntegerInteger),
        (SemanticType::Float, SemanticType::Float) => Ok(NcsAuxCode::TypeTypeFloatFloat),
        (SemanticType::Object, SemanticType::Object) => Ok(NcsAuxCode::TypeTypeObjectObject),
        (SemanticType::String, SemanticType::String) => Ok(NcsAuxCode::TypeTypeStringString),
        (SemanticType::Struct(_), SemanticType::Struct(_)) => Ok(NcsAuxCode::TypeTypeStructStruct),
        (SemanticType::Int, SemanticType::Float) => Ok(NcsAuxCode::TypeTypeIntegerFloat),
        (SemanticType::Float, SemanticType::Int) => Ok(NcsAuxCode::TypeTypeFloatInteger),
        (SemanticType::Vector, SemanticType::Vector) => Ok(NcsAuxCode::TypeTypeVectorVector),
        (SemanticType::Vector, SemanticType::Float) => Ok(NcsAuxCode::TypeTypeVectorFloat),
        (SemanticType::Float, SemanticType::Vector) => Ok(NcsAuxCode::TypeTypeFloatVector),
        (SemanticType::EngineStructure(name), SemanticType::EngineStructure(other))
            if name == other =>
        {
            aux_for_engine_structure(name, langspec).and_then(|left_aux| match left_aux {
                NcsAuxCode::TypeEngst0 => Ok(NcsAuxCode::TypeTypeEngst0Engst0),
                NcsAuxCode::TypeEngst1 => Ok(NcsAuxCode::TypeTypeEngst1Engst1),
                NcsAuxCode::TypeEngst2 => Ok(NcsAuxCode::TypeTypeEngst2Engst2),
                NcsAuxCode::TypeEngst3 => Ok(NcsAuxCode::TypeTypeEngst3Engst3),
                NcsAuxCode::TypeEngst4 => Ok(NcsAuxCode::TypeTypeEngst4Engst4),
                NcsAuxCode::TypeEngst5 => Ok(NcsAuxCode::TypeTypeEngst5Engst5),
                NcsAuxCode::TypeEngst6 => Ok(NcsAuxCode::TypeTypeEngst6Engst6),
                NcsAuxCode::TypeEngst7 => Ok(NcsAuxCode::TypeTypeEngst7Engst7),
                NcsAuxCode::TypeEngst8 => Ok(NcsAuxCode::TypeTypeEngst8Engst8),
                NcsAuxCode::TypeEngst9 => Ok(NcsAuxCode::TypeTypeEngst9Engst9),
                _ => Err(CodegenError::new(None, "invalid engine-structure auxcode")),
            })
        }
        _ => Err(CodegenError::new(
            None,
            format!("unsupported binary operand pair for code generation: {left:?} and {right:?}"),
        )),
    }
}

pub(super) fn aux_for_unary(
    ty: &SemanticType,
    langspec: Option<&LangSpec>,
) -> Result<NcsAuxCode, CodegenError> {
    match ty {
        SemanticType::Int => Ok(NcsAuxCode::TypeInteger),
        SemanticType::Float => Ok(NcsAuxCode::TypeFloat),
        SemanticType::String => Ok(NcsAuxCode::TypeString),
        SemanticType::Object => Ok(NcsAuxCode::TypeObject),
        SemanticType::Vector => Ok(NcsAuxCode::TypeTypeVectorVector),
        SemanticType::EngineStructure(name) => aux_for_engine_structure(name, langspec),
        SemanticType::Struct(_) => Ok(NcsAuxCode::TypeTypeStructStruct),
        SemanticType::Enum {
            backing, ..
        } => aux_for_unary(&enum_backing_semantic_type(*backing), langspec),
        SemanticType::Void | SemanticType::Action => Err(CodegenError::new(
            None,
            format!("unsupported unary operand type {ty:?}"),
        )),
    }
}

pub(super) fn aux_for_engine_structure(
    name: &str,
    langspec: Option<&LangSpec>,
) -> Result<NcsAuxCode, CodegenError> {
    let index = usize::from(engine_structure_index(name, langspec)?);

    Ok(match index {
        0 => NcsAuxCode::TypeEngst0,
        1 => NcsAuxCode::TypeEngst1,
        2 => NcsAuxCode::TypeEngst2,
        3 => NcsAuxCode::TypeEngst3,
        4 => NcsAuxCode::TypeEngst4,
        5 => NcsAuxCode::TypeEngst5,
        6 => NcsAuxCode::TypeEngst6,
        7 => NcsAuxCode::TypeEngst7,
        8 => NcsAuxCode::TypeEngst8,
        9 => NcsAuxCode::TypeEngst9,
        _ => {
            return Err(CodegenError::new(
                None,
                format!("engine structure index out of range for {name:?}"),
            ));
        }
    })
}

pub(super) fn size_of_type(
    ty: &SemanticType,
    structs: &BTreeMap<String, &crate::HirStruct>,
) -> Result<usize, CodegenError> {
    match ty {
        SemanticType::Void | SemanticType::Action => Ok(0),
        SemanticType::Int
        | SemanticType::Float
        | SemanticType::String
        | SemanticType::Object
        | SemanticType::EngineStructure(_)
        | SemanticType::Enum {
            backing: crate::EnumBackingType::Int,
            ..
        }
        | SemanticType::Enum {
            backing: crate::EnumBackingType::String,
            ..
        } => Ok(4),
        SemanticType::Vector => Ok(12),
        SemanticType::Struct(name) => {
            let structure = structs
                .get(name)
                .ok_or_else(|| CodegenError::new(None, format!("unknown structure {name:?}")))?;
            let mut size = 0usize;
            for field in &structure.fields {
                size += size_of_type(&field.ty, structs)?;
            }
            Ok(size)
        }
    }
}

pub(super) fn validate_hir_limits(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
) -> Result<(), CodegenError> {
    let builtin_identifiers = langspec.map_or(0, |spec| {
        spec.constants.len() + spec.functions.len() + spec.engine_structures.len()
    });
    let user_identifiers = hir.globals.len()
        + hir
            .structs
            .iter()
            .map(|structure| 1 + structure.fields.len())
            .sum::<usize>()
        + hir
            .functions
            .iter()
            .map(|function| 1 + function.locals.len())
            .sum::<usize>();
    let identifier_count = builtin_identifiers.saturating_add(user_identifiers);
    if identifier_count >= MAX_COMPILER_IDENTIFIERS {
        let span = hir
            .functions
            .last()
            .map(|function| function.span)
            .or_else(|| hir.globals.last().map(|global| global.span));
        return Err(CodegenError::native(
            crate::CompilerErrorCode::IdentifierListFull,
            span,
            format!(
                "compiler identifier table contains {identifier_count} entries; the native limit \
                 is {}",
                MAX_COMPILER_IDENTIFIERS - 1
            ),
        ));
    }

    let structs = hir
        .structs
        .iter()
        .map(|structure| (structure.name.clone(), structure))
        .collect::<BTreeMap<_, _>>();
    let global_bytes = hir.globals.iter().try_fold(0usize, |total, global| {
        Ok::<_, CodegenError>(total.saturating_add(size_of_type(&global.ty, &structs)?))
    })?;
    for function in hir
        .functions
        .iter()
        .filter(|function| function.body.is_some())
    {
        let return_bytes = if function.return_type == SemanticType::Void {
            0
        } else {
            size_of_type(&function.return_type, &structs)?
        };
        let frame_bytes = function
            .locals
            .iter()
            .try_fold(return_bytes, |total, local| {
                Ok::<_, CodegenError>(total.saturating_add(size_of_type(&local.ty, &structs)?))
            })?;
        let runtime_cells = global_bytes.saturating_add(frame_bytes).div_ceil(4);
        if runtime_cells > MAX_COMPILER_RUNTIME_CELLS {
            return Err(CodegenError::native(
                crate::CompilerErrorCode::ScriptTooLarge,
                Some(function.span),
                format!(
                    "function {:?} requires {runtime_cells} runtime cells with globals; the \
                     native compiler limit is {MAX_COMPILER_RUNTIME_CELLS}",
                    function.name
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn field_layout(
    base: &SemanticType,
    field: &str,
    structs: &BTreeMap<String, &crate::HirStruct>,
    span: Option<crate::Span>,
) -> Result<FieldLayout, CodegenError> {
    match base {
        SemanticType::Vector => {
            let offset = match field {
                "x" => 0,
                "y" => 4,
                "z" => 8,
                _ => {
                    return Err(CodegenError::new(
                        span,
                        format!("field {field:?} does not exist on vector"),
                    ));
                }
            };
            Ok(FieldLayout {
                ty: SemanticType::Float,
                offset,
                size: 4,
            })
        }
        SemanticType::Struct(name) => {
            let structure = structs
                .get(name)
                .ok_or_else(|| CodegenError::new(span, format!("unknown structure {name:?}")))?;
            let mut offset = 0usize;
            for candidate in &structure.fields {
                let size = size_of_type(&candidate.ty, structs)?;
                if candidate.name == field {
                    return Ok(FieldLayout {
                        ty: candidate.ty.clone(),
                        offset,
                        size,
                    });
                }
                offset += size;
            }
            Err(CodegenError::new(
                span,
                format!("field {field:?} does not exist on structure {name:?}"),
            ))
        }
        _ => Err(CodegenError::new(
            span,
            format!("field access requires a vector or struct base, got {base:?}"),
        )),
    }
}

pub(super) enum AssignmentTargetRoot<'a> {
    Local(HirLocalId),
    Global(&'a str),
}

pub(super) struct AssignmentTarget<'a> {
    pub(super) root:   AssignmentTargetRoot<'a>,
    pub(super) offset: usize,
    pub(super) size:   usize,
}

pub(super) fn resolve_assignment_target<'a>(
    target: &'a HirExpr,
    structs: &BTreeMap<String, &'a crate::HirStruct>,
    span: Option<crate::Span>,
) -> Result<AssignmentTarget<'a>, CodegenError> {
    match &target.kind {
        HirExprKind::Value(crate::HirValueRef::Local(local)) => Ok(AssignmentTarget {
            root:   AssignmentTargetRoot::Local(*local),
            offset: 0,
            size:   size_of_type(&target.ty, structs)?,
        }),
        HirExprKind::Value(
            crate::HirValueRef::Global(name) | crate::HirValueRef::ConstGlobal(name),
        ) => Ok(AssignmentTarget {
            root:   AssignmentTargetRoot::Global(name),
            offset: 0,
            size:   size_of_type(&target.ty, structs)?,
        }),
        HirExprKind::FieldAccess {
            base,
            field,
        } => {
            let mut resolved = resolve_assignment_target(base, structs, span)?;
            let field_layout = field_layout(&base.ty, field, structs, span)?;
            resolved.offset += field_layout.offset;
            resolved.size = field_layout.size;
            Ok(resolved)
        }
        _ => Err(CodegenError::new(
            span,
            "assignment target code generation is not implemented yet",
        )),
    }
}

pub(super) fn string_extra(value: &ScriptString) -> Result<Vec<u8>, CodegenError> {
    string_extra_bytes(value.as_bytes())
}

pub(super) fn string_extra_bytes(value: &[u8]) -> Result<Vec<u8>, CodegenError> {
    let length = u16::try_from(value.len()).map_err(|_error| {
        CodegenError::new(None, "string constant exceeds NCS 16-bit length limit")
    })?;
    let mut bytes = Vec::with_capacity(2 + value.len());
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value);
    Ok(bytes)
}
