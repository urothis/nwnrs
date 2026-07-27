use super::{super::*, values::*};

pub(in crate::codegen) fn hir_expr_contains_match(expr: &HirExpr) -> bool {
    match &expr.kind {
        HirExprKind::Match {
            ..
        }
        | HirExprKind::CheckedEnumConversion {
            ..
        } => true,
        HirExprKind::Literal(_) | HirExprKind::Value(_) => false,
        HirExprKind::Call {
            arguments, ..
        } => arguments.iter().any(hir_expr_contains_match),
        HirExprKind::FieldAccess {
            base, ..
        }
        | HirExprKind::Unary {
            expr: base, ..
        } => hir_expr_contains_match(base),
        HirExprKind::Binary {
            left,
            right,
            ..
        }
        | HirExprKind::Assignment {
            left,
            right,
            ..
        } => hir_expr_contains_match(left) || hir_expr_contains_match(right),
        HirExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            hir_expr_contains_match(condition)
                || hir_expr_contains_match(when_true)
                || hir_expr_contains_match(when_false)
        }
    }
}

pub(in crate::codegen) fn emit_expr_common(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    expr: &HirExpr,
) -> Result<(), CodegenError> {
    // Constant folding is part of the native compiler's ordinary parse-tree
    // walk, independent of its selectable optimization flags.
    if matches!(
        &expr.kind,
        HirExprKind::Unary { .. } | HirExprKind::Binary { .. } | HirExprKind::Match { .. }
    ) && let Some(value) = evaluate_const_expr(expr, &compiler.constant_env)
    {
        let literal = match value {
            ConstValue::Int(value) => Literal::Integer(value),
            ConstValue::Float(value) => Literal::Float(value),
            ConstValue::String(value) => Literal::String(value),
        };
        return emit_push_literal(compiler, temp_bytes, &literal, &expr.ty, Some(expr.span));
    }

    match &expr.kind {
        HirExprKind::Match {
            ..
        } => Err(CodegenError::new(
            Some(expr.span),
            "match expression reached code generation before control-flow lowering",
        )),
        HirExprKind::CheckedEnumConversion {
            ..
        } => Err(CodegenError::new(
            Some(expr.span),
            "checked enum conversion reached code generation before control-flow lowering",
        )),
        HirExprKind::Literal(literal) => {
            emit_push_literal(compiler, temp_bytes, literal, &expr.ty, Some(expr.span))
        }
        HirExprKind::Value(value) => match value {
            crate::HirValueRef::Local(local) => {
                let layout = layout.ok_or_else(|| {
                    CodegenError::new(Some(expr.span), "local value used outside a function")
                })?;
                let slot = layout.locals.get(local).ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown local slot {local:?}"))
                })?;
                let frame_bytes = function_frame_bytes(layout);
                let offset = usize_to_i32(slot.offset, "local load offset")?
                    - usize_to_i32(frame_bytes + *temp_bytes, "local frame bytes")?;
                compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::RunstackCopy,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra:   assignment_extra(offset, slot.size)?,
                });
                *temp_bytes += slot.size;
                Ok(())
            }
            crate::HirValueRef::Global(name) => {
                let slot = compiler.global_layout.get(name).ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown global {name:?}"))
                })?;
                let (opcode, stack_size) = compiler.global_init_bytes.map_or(
                    (NcsOpcode::RunstackCopyBase, compiler.global_size),
                    |allocated| (NcsOpcode::RunstackCopy, allocated + *temp_bytes),
                );
                let offset = usize_to_i32(slot.offset, "global load offset")?
                    - usize_to_i32(stack_size, "global load stack size")?;
                compiler.assembler.push(NcsInstruction {
                    opcode,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra: assignment_extra(offset, slot.size)?,
                });
                *temp_bytes += slot.size;
                Ok(())
            }
            crate::HirValueRef::ConstGlobal(name) => {
                let value = compiler.constant_env.get(name).ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown const global {name:?}"))
                })?;
                let literal = match value {
                    ConstValue::Int(value) => Literal::Integer(*value),
                    ConstValue::Float(value) => Literal::Float(*value),
                    ConstValue::String(value) => Literal::String(value.clone()),
                };
                emit_push_literal(compiler, temp_bytes, &literal, &expr.ty, Some(expr.span))
            }
            crate::HirValueRef::BuiltinConstant(name) => {
                let value = compiler.builtin_constants.get(name).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unknown builtin constant {name:?}"),
                    )
                })?;
                let literal = literal_from_builtin_value(value).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unsupported builtin constant value for {name:?}"),
                    )
                })?;
                emit_push_literal(compiler, temp_bytes, &literal, &expr.ty, Some(expr.span))
            }
        },
        HirExprKind::Call {
            target,
            arguments,
        } => emit_call(compiler, temp_bytes, layout, expr, target, arguments),
        HirExprKind::FieldAccess {
            base,
            field,
        } => {
            emit_expr_common(compiler, temp_bytes, layout, base)?;
            let base_size = size_of_type(&base.ty, &compiler.structs)?;
            let field_layout = field_layout(&base.ty, field, &compiler.structs, Some(expr.span))?;
            debug_assert_eq!(
                size_of_type(&field_layout.ty, &compiler.structs)?,
                size_of_type(&expr.ty, &compiler.structs)?
            );
            let mut extra = Vec::with_capacity(6);
            extra.extend_from_slice(&usize_to_u16(base_size, "structure size")?.to_be_bytes());
            extra.extend_from_slice(
                &usize_to_u16(field_layout.offset, "structure field offset")?.to_be_bytes(),
            );
            extra.extend_from_slice(
                &usize_to_u16(field_layout.size, "structure field size")?.to_be_bytes(),
            );
            compiler.assembler.push(NcsInstruction {
                opcode: NcsOpcode::DeStruct,
                auxcode: NcsAuxCode::TypeVoid,
                extra,
            });
            *temp_bytes = temp_bytes.saturating_sub(base_size);
            *temp_bytes += field_layout.size;
            Ok(())
        }
        HirExprKind::Unary {
            op,
            expr: inner,
        } => {
            if matches!(
                op,
                UnaryOp::PreIncrement
                    | UnaryOp::PreDecrement
                    | UnaryOp::PostIncrement
                    | UnaryOp::PostDecrement
            ) {
                if matches!(op, UnaryOp::PreIncrement | UnaryOp::PreDecrement) {
                    emit_increment_target(compiler, *temp_bytes, layout, inner, *op, expr.span)?;
                    emit_expr_common(compiler, temp_bytes, layout, inner)?;
                    return Ok(());
                }
                emit_expr_common(compiler, temp_bytes, layout, inner)?;
                emit_increment_target(compiler, *temp_bytes, layout, inner, *op, expr.span)?;
                return Ok(());
            }
            emit_expr_common(compiler, temp_bytes, layout, inner)?;
            let opcode = match op {
                UnaryOp::Negate => NcsOpcode::Negation,
                UnaryOp::OnesComplement => NcsOpcode::OnesComplement,
                UnaryOp::BooleanNot => NcsOpcode::BooleanNot,
                UnaryOp::PreIncrement
                | UnaryOp::PreDecrement
                | UnaryOp::PostIncrement
                | UnaryOp::PostDecrement => {
                    return Err(CodegenError::new(
                        Some(expr.span),
                        "increment expression reached ordinary unary code generation",
                    ));
                }
            };
            compiler.assembler.push(NcsInstruction {
                opcode,
                auxcode: aux_for_unary(&expr.ty, compiler.langspec)?,
                extra: Vec::new(),
            });
            Ok(())
        }
        HirExprKind::Binary {
            op,
            left,
            right,
        } => {
            emit_expr_common(compiler, temp_bytes, layout, left)?;
            if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                let base_temp_bytes = temp_bytes.saturating_sub(4);
                emit_copy_top_bytes(compiler, temp_bytes, 4)?;
                let short_circuit = compiler.assembler.new_label();
                *temp_bytes = temp_bytes.saturating_sub(4);
                compiler.assembler.push_jump(NcsOpcode::Jz, short_circuit);

                if *op == BinaryOp::LogicalOr {
                    emit_copy_top_bytes(compiler, temp_bytes, 4)?;
                    let merge = compiler.assembler.new_label();
                    compiler.assembler.push_jump(NcsOpcode::Jmp, merge);
                    compiler.assembler.place_label(short_circuit);
                    *temp_bytes = base_temp_bytes + 4;
                    emit_expr_common(compiler, temp_bytes, layout, right)?;
                    compiler.assembler.place_label(merge);
                } else {
                    emit_expr_common(compiler, temp_bytes, layout, right)?;
                }

                let opcode = opcode_for_binary(*op);
                *temp_bytes = temp_bytes.saturating_sub(8) + 4;
                compiler.assembler.push(NcsInstruction {
                    opcode,
                    auxcode: NcsAuxCode::TypeTypeIntegerInteger,
                    extra: Vec::new(),
                });
                if *op == BinaryOp::LogicalAnd {
                    compiler.assembler.place_label(short_circuit);
                }
                return Ok(());
            }
            emit_expr_common(compiler, temp_bytes, layout, right)?;
            let opcode = opcode_for_binary(*op);
            let aux = aux_for_binary(&left.ty, &right.ty, compiler.langspec)?;
            let left_size = size_of_type(&left.ty, &compiler.structs)?;
            let right_size = size_of_type(&right.ty, &compiler.structs)?;
            let result_size = size_of_binary_result(*op, &left.ty, &right.ty, &compiler.structs)?;
            *temp_bytes = temp_bytes.saturating_sub(left_size + right_size);
            *temp_bytes += result_size;
            let extra = if aux == NcsAuxCode::TypeTypeStructStruct
                && matches!(opcode, NcsOpcode::Equal | NcsOpcode::NotEqual)
            {
                usize_to_u16(left_size, "struct equality size")?
                    .to_be_bytes()
                    .to_vec()
            } else {
                Vec::new()
            };
            compiler.assembler.push(NcsInstruction {
                opcode,
                auxcode: aux,
                extra,
            });
            Ok(())
        }
        HirExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            let base_temp_bytes = *temp_bytes;
            emit_expr_common(compiler, temp_bytes, layout, condition)?;
            if *temp_bytes < base_temp_bytes + 4 {
                return Err(CodegenError::new(
                    Some(condition.span),
                    "conditional expression requires an integer condition",
                ));
            }

            let false_label = compiler.assembler.new_label();
            let end_label = compiler.assembler.new_label();
            *temp_bytes -= 4;
            compiler.assembler.push_jump(NcsOpcode::Jz, false_label);

            emit_expr_common(compiler, temp_bytes, layout, when_true)?;
            compiler.assembler.push_jump(NcsOpcode::Jmp, end_label);

            compiler.assembler.place_label(false_label);
            *temp_bytes = base_temp_bytes;
            emit_expr_common(compiler, temp_bytes, layout, when_false)?;
            compiler.assembler.place_label(end_label);
            Ok(())
        }
        HirExprKind::Assignment {
            op,
            left,
            right,
        } => {
            if *op == AssignmentOp::Assign {
                emit_expr_common(compiler, temp_bytes, layout, right)?;
                emit_store_target(compiler, temp_bytes, layout, left, right.span)?;
                return Ok(());
            }

            let binary_op = match op {
                AssignmentOp::Assign => {
                    return Err(CodegenError::new(
                        Some(expr.span),
                        "plain assignment reached compound-assignment code generation",
                    ));
                }
                AssignmentOp::AssignMinus => BinaryOp::Subtract,
                AssignmentOp::AssignPlus => BinaryOp::Add,
                AssignmentOp::AssignMultiply => BinaryOp::Multiply,
                AssignmentOp::AssignDivide => BinaryOp::Divide,
                AssignmentOp::AssignModulus => BinaryOp::Modulus,
                AssignmentOp::AssignAnd => BinaryOp::BooleanAnd,
                AssignmentOp::AssignXor => BinaryOp::ExclusiveOr,
                AssignmentOp::AssignOr => BinaryOp::InclusiveOr,
                AssignmentOp::AssignShiftLeft => BinaryOp::ShiftLeft,
                AssignmentOp::AssignShiftRight => BinaryOp::ShiftRight,
                AssignmentOp::AssignUnsignedShiftRight => BinaryOp::UnsignedShiftRight,
            };
            emit_expr_common(compiler, temp_bytes, layout, left)?;
            emit_expr_common(compiler, temp_bytes, layout, right)?;
            let aux = aux_for_binary(&left.ty, &right.ty, compiler.langspec)?;
            let left_size = size_of_type(&left.ty, &compiler.structs)?;
            let right_size = size_of_type(&right.ty, &compiler.structs)?;
            let result_size =
                size_of_binary_result(binary_op, &left.ty, &right.ty, &compiler.structs)?;
            *temp_bytes = temp_bytes.saturating_sub(left_size + right_size);
            *temp_bytes += result_size;
            compiler.assembler.push(NcsInstruction {
                opcode:  opcode_for_binary(binary_op),
                auxcode: aux,
                extra:   Vec::new(),
            });
            emit_store_target(compiler, temp_bytes, layout, left, expr.span)
        }
    }
}

pub(in crate::codegen) fn emit_call(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    expr: &HirExpr,
    target: &HirCallTarget,
    arguments: &[HirExpr],
) -> Result<(), CodegenError> {
    let base_temp = *temp_bytes;
    match target {
        HirCallTarget::Builtin(name) => {
            let (id, function) =
                compiler
                    .builtin_functions
                    .get(name)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::new(Some(expr.span), format!("unknown builtin {name:?}"))
                    })?;
            // ACTION handlers pop parameters in declaration order, so the
            // compiler must place the last parameter on the stack first.
            // Action parameters do not occupy a normal stack cell, but they
            // still participate in this right-to-left emission order.
            for (index, parameter) in function.parameters.iter().enumerate().rev() {
                if let Some(argument) = arguments.get(index) {
                    if matches!(parameter.ty, BuiltinType::Action) {
                        emit_action_parameter(compiler, temp_bytes, layout, argument)?;
                    } else {
                        emit_expr_common(compiler, temp_bytes, layout, argument)?;
                    }
                    continue;
                }

                let default = parameter.default.as_ref().ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("missing required parameter for builtin {name:?}"),
                    )
                })?;
                if matches!(parameter.ty, BuiltinType::Action) {
                    let action = lower_builtin_action_default_expr(compiler, default, expr.span)?;
                    emit_action_parameter(compiler, temp_bytes, layout, &action)?;
                    continue;
                }
                if matches!(parameter.ty, BuiltinType::Object)
                    && let BuiltinValue::ObjectId(value) = default
                {
                    // The legacy langspec spells its invalid-object default as
                    // the integer OBJECT_TYPE_INVALID (32767), but native
                    // codegen widens that sentinel to INVALID_OBJECT_ID.
                    let object_id = if *value == 32_767 {
                        0x7f00_0000
                    } else {
                        *value
                    };
                    compiler.assembler.push(NcsInstruction {
                        opcode:  NcsOpcode::Constant,
                        auxcode: NcsAuxCode::TypeObject,
                        extra:   object_id.to_be_bytes().to_vec(),
                    });
                    *temp_bytes += 4;
                    continue;
                }
                let literal = literal_from_builtin_value(default).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unsupported builtin default value for {name:?}"),
                    )
                })?;
                let ty = semantic_type_from_builtin_type(&parameter.ty);
                emit_push_literal(compiler, temp_bytes, &literal, &ty, Some(expr.span))?;
            }

            let return_size = size_of_type(&expr.ty, &compiler.structs)?;
            *temp_bytes = base_temp + return_size;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ExecuteCommand,
                auxcode: NcsAuxCode::None,
                extra:   builtin_call_extra(
                    id,
                    usize_to_u8(function.parameters.len(), "builtin argc")?,
                ),
            });
            Ok(())
        }
        HirCallTarget::Function(name) => {
            let function = compiler.functions.get(name).copied().ok_or_else(|| {
                CodegenError::new(Some(expr.span), format!("unknown function {name:?}"))
            })?;
            if function.return_type != SemanticType::Void {
                compiler.emit_stack_alloc(&function.return_type)?;
                *temp_bytes += size_of_type(&function.return_type, &compiler.structs)?;
            }
            for (index, parameter) in function.parameters.iter().enumerate().rev() {
                if let Some(argument) = arguments.get(index) {
                    emit_expr_common(compiler, temp_bytes, layout, argument)?;
                } else {
                    let default = parameter.default.as_ref().ok_or_else(|| {
                        CodegenError::new(
                            Some(expr.span),
                            format!("missing required parameter for function {name:?}"),
                        )
                    })?;
                    emit_expr_common(compiler, temp_bytes, layout, default)?;
                }
            }
            let label = compiler.function_labels.get(name).copied().ok_or_else(|| {
                CodegenError::new(
                    Some(expr.span),
                    format!("missing function label for {name:?}"),
                )
            })?;
            compiler.assembler.push_jump(NcsOpcode::Jsr, label);
            let return_size = size_of_type(&function.return_type, &compiler.structs)?;
            *temp_bytes = base_temp + return_size;
            Ok(())
        }
    }
}

pub(in crate::codegen) fn emit_action_parameter(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    argument: &HirExpr,
) -> Result<(), CodegenError> {
    let stack_bytes = layout.map_or(0, function_frame_bytes) + *temp_bytes;

    // Upstream emits STORESTATE with aux byte 0x10 before a JMP over the
    // embedded action body. Our NCS model does not have a dedicated STORESTATE
    // aux variant, so we preserve the raw byte value via the matching enum
    // discriminant.
    compiler.assembler.push(NcsInstruction {
        opcode:  NcsOpcode::StoreState,
        auxcode: NcsAuxCode::TypeEngst0,
        extra:   store_state_extra(
            usize_to_u32(compiler.global_size, "global size")?,
            usize_to_u32(stack_bytes, "stack size")?,
        ),
    });

    let action_end = compiler.assembler.new_label();
    compiler.assembler.push_jump(NcsOpcode::Jmp, action_end);
    emit_expr_common(compiler, temp_bytes, layout, argument)?;
    compiler.assembler.push(simple_instruction(NcsOpcode::Ret));
    compiler.assembler.place_label(action_end);
    Ok(())
}

pub(in crate::codegen) fn lower_builtin_action_default_expr(
    compiler: &O0Compiler<'_>,
    default: &BuiltinValue,
    span: crate::Span,
) -> Result<HirExpr, CodegenError> {
    let BuiltinValue::Raw(raw) = default else {
        return Err(CodegenError::new(
            Some(span),
            format!("unsupported builtin action default value {default:?}"),
        ));
    };
    let langspec = compiler.langspec.ok_or_else(|| {
        CodegenError::new(
            Some(span),
            "builtin action defaults require an active langspec".to_string(),
        )
    })?;
    let synthetic = format!("void __nwnrs_builtin_action_default__() {{ {raw}; }}");
    let script =
        parse_text(SourceId::new(u32::MAX - 1), &synthetic, Some(langspec)).map_err(|error| {
            CodegenError::new(
                Some(span),
                format!("failed to parse builtin action default {raw:?}: {error}"),
            )
        })?;
    let semantic = analyze_script_with_options(&script, Some(langspec), SemanticOptions::default())
        .map_err(|error| {
            CodegenError::new(
                Some(span),
                format!("failed to analyze builtin action default {raw:?}: {error}"),
            )
        })?;
    let hir = lower_to_hir(&script, &semantic, Some(langspec)).map_err(|error| {
        CodegenError::new(
            Some(span),
            format!("failed to lower builtin action default {raw:?}: {error}"),
        )
    })?;
    let function = hir.functions.first().ok_or_else(|| {
        CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} did not lower to a function body"),
        )
    })?;
    let body = function.body.as_ref().ok_or_else(|| {
        CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} lowered without a function body"),
        )
    })?;
    let statement = body.statements.first().ok_or_else(|| {
        CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} lowered to an empty body"),
        )
    })?;
    match statement {
        HirStmt::Expr(expr) => Ok((*expr.clone()).clone()),
        _ => Err(CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} must lower to an expression statement"),
        )),
    }
}

pub(in crate::codegen) fn emit_store_target(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    target: &HirExpr,
    span: crate::Span,
) -> Result<(), CodegenError> {
    let resolved = resolve_assignment_target(target, &compiler.structs, Some(span))?;
    match resolved.root {
        AssignmentTargetRoot::Local(local) => {
            let layout = layout.ok_or_else(|| {
                CodegenError::new(Some(span), "local assignment used outside a function")
            })?;
            let slot = layout.locals.get(&local).ok_or_else(|| {
                CodegenError::new(Some(span), format!("unknown local slot {local:?}"))
            })?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "local assignment offset")?
                - usize_to_i32(
                    function_frame_bytes(layout) + *temp_bytes,
                    "local assignment frame size",
                )?;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::Assignment,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   assignment_extra(offset, resolved.size)?,
            });
            Ok(())
        }
        AssignmentTargetRoot::Global(name) => {
            let slot = compiler
                .global_layout
                .get(name)
                .ok_or_else(|| CodegenError::new(Some(span), format!("unknown global {name:?}")))?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "global assignment offset")?
                - usize_to_i32(compiler.global_size, "global size")?;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::AssignmentBase,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   assignment_extra(offset, resolved.size)?,
            });
            Ok(())
        }
    }
}

pub(in crate::codegen) fn emit_increment_target(
    compiler: &mut O0Compiler<'_>,
    stack_bytes: usize,
    layout: Option<&FunctionLayout>,
    target: &HirExpr,
    op: UnaryOp,
    span: crate::Span,
) -> Result<(), CodegenError> {
    let resolved = resolve_assignment_target(target, &compiler.structs, Some(span))?;
    let increment = matches!(op, UnaryOp::PreIncrement | UnaryOp::PostIncrement);
    let (opcode, offset) = match resolved.root {
        AssignmentTargetRoot::Local(local) => {
            let layout = layout.ok_or_else(|| {
                CodegenError::new(Some(span), "local increment used outside a function")
            })?;
            let slot = layout.locals.get(&local).ok_or_else(|| {
                CodegenError::new(Some(span), format!("unknown local slot {local:?}"))
            })?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "local increment offset")?
                - usize_to_i32(
                    function_frame_bytes(layout) + stack_bytes,
                    "local increment frame size",
                )?;
            let opcode = if increment {
                NcsOpcode::Increment
            } else {
                NcsOpcode::Decrement
            };
            (opcode, offset)
        }
        AssignmentTargetRoot::Global(name) => {
            let slot = compiler
                .global_layout
                .get(name)
                .ok_or_else(|| CodegenError::new(Some(span), format!("unknown global {name:?}")))?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "global increment offset")?
                - usize_to_i32(compiler.global_size, "global size")?;
            let opcode = if increment {
                NcsOpcode::IncrementBase
            } else {
                NcsOpcode::DecrementBase
            };
            (opcode, offset)
        }
    };
    compiler.assembler.push(NcsInstruction {
        opcode,
        auxcode: NcsAuxCode::TypeInteger,
        extra: offset.to_be_bytes().to_vec(),
    });
    Ok(())
}
