use super::*;

impl FunctionEmitter<'_, '_> {
    pub(super) fn emit_expr(&mut self, expr: &HirExpr) -> Result<(), CodegenError> {
        if hir_expr_contains_match(expr) {
            return self.emit_expr_with_match(expr);
        }
        emit_expr_common(
            self.compiler,
            &mut self.temp_bytes,
            Some(&self.layout),
            expr,
        )
    }

    pub(super) fn emit_expr_with_match(&mut self, expr: &HirExpr) -> Result<(), CodegenError> {
        self.compiler.check_cancelled(Some(expr.span))?;
        match &expr.kind {
            HirExprKind::Match {
                value,
                arms,
            } => self.emit_match_expr(expr, value, arms),
            HirExprKind::CheckedEnumConversion {
                value,
                fallback,
                valid_values,
            } => self.emit_checked_enum_conversion(expr, value, fallback, valid_values),
            HirExprKind::FieldAccess {
                base,
                field,
            } => {
                self.emit_expr(base)?;
                let base_size = size_of_type(&base.ty, &self.compiler.structs)?;
                let field_layout =
                    field_layout(&base.ty, field, &self.compiler.structs, Some(expr.span))?;
                let mut extra = Vec::with_capacity(6);
                extra.extend_from_slice(&usize_to_u16(base_size, "structure size")?.to_be_bytes());
                extra.extend_from_slice(
                    &usize_to_u16(field_layout.offset, "structure field offset")?.to_be_bytes(),
                );
                extra.extend_from_slice(
                    &usize_to_u16(field_layout.size, "structure field size")?.to_be_bytes(),
                );
                self.compiler.assembler.push(NcsInstruction {
                    opcode: NcsOpcode::DeStruct,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra,
                });
                self.temp_bytes = self.temp_bytes.saturating_sub(base_size);
                self.temp_bytes += field_layout.size;
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
                    return Err(CodegenError::new(
                        Some(expr.span),
                        "a match expression cannot be used as an increment target",
                    ));
                }
                self.emit_expr(inner)?;
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
                self.compiler.assembler.push(NcsInstruction {
                    opcode,
                    auxcode: aux_for_unary(&expr.ty, self.compiler.langspec)?,
                    extra: Vec::new(),
                });
                Ok(())
            }
            HirExprKind::Binary {
                op,
                left,
                right,
            } => {
                self.emit_expr(left)?;
                if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    let base_temp_bytes = self.temp_bytes.saturating_sub(4);
                    self.emit_copy_top_value(4)?;
                    let short_circuit = self.compiler.assembler.new_label();
                    self.temp_bytes = self.temp_bytes.saturating_sub(4);
                    self.compiler
                        .assembler
                        .push_jump(NcsOpcode::Jz, short_circuit);

                    if *op == BinaryOp::LogicalOr {
                        self.emit_copy_top_value(4)?;
                        let merge = self.compiler.assembler.new_label();
                        self.compiler.assembler.push_jump(NcsOpcode::Jmp, merge);
                        self.compiler.assembler.place_label(short_circuit);
                        self.temp_bytes = base_temp_bytes + 4;
                        self.emit_expr(right)?;
                        self.compiler.assembler.place_label(merge);
                    } else {
                        self.emit_expr(right)?;
                    }

                    self.temp_bytes = self.temp_bytes.saturating_sub(8) + 4;
                    self.compiler.assembler.push(NcsInstruction {
                        opcode:  opcode_for_binary(*op),
                        auxcode: NcsAuxCode::TypeTypeIntegerInteger,
                        extra:   Vec::new(),
                    });
                    if *op == BinaryOp::LogicalAnd {
                        self.compiler.assembler.place_label(short_circuit);
                    }
                    return Ok(());
                }
                self.emit_expr(right)?;
                self.emit_binary(*op, &left.ty, &right.ty, Some(expr.span))
            }
            HirExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                let base_temp_bytes = self.temp_bytes;
                self.emit_expr(condition)?;
                let false_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.emit_branch_zero(false_label)?;
                self.emit_expr(when_true)?;
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, end_label);
                self.compiler.assembler.place_label(false_label);
                self.temp_bytes = base_temp_bytes;
                self.emit_expr(when_false)?;
                self.compiler.assembler.place_label(end_label);
                Ok(())
            }
            HirExprKind::Assignment {
                op,
                left,
                right,
            } => {
                if hir_expr_contains_match(left) {
                    return Err(CodegenError::new(
                        Some(left.span),
                        "a match expression cannot be used as an assignment target",
                    ));
                }
                if *op == AssignmentOp::Assign {
                    self.emit_expr(right)?;
                    emit_store_target(
                        self.compiler,
                        &mut self.temp_bytes,
                        Some(&self.layout),
                        left,
                        right.span,
                    )?;
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
                self.emit_expr(left)?;
                self.emit_expr(right)?;
                self.emit_binary(binary_op, &left.ty, &right.ty, Some(expr.span))?;
                emit_store_target(
                    self.compiler,
                    &mut self.temp_bytes,
                    Some(&self.layout),
                    left,
                    expr.span,
                )
            }
            HirExprKind::Call {
                target,
                arguments,
            } => self.emit_call_with_match(expr, target, arguments),
            HirExprKind::Literal(_) | HirExprKind::Value(_) => emit_expr_common(
                self.compiler,
                &mut self.temp_bytes,
                Some(&self.layout),
                expr,
            ),
        }
    }

    pub(super) fn emit_match_expr(
        &mut self,
        expression: &HirExpr,
        value: &HirExpr,
        arms: &[crate::HirMatchArm],
    ) -> Result<(), CodegenError> {
        let base_temp_bytes = self.temp_bytes;
        self.emit_expr(value)?;
        let value_size = size_of_type(&value.ty, &self.compiler.structs)?;
        let result_size = size_of_type(&expression.ty, &self.compiler.structs)?;
        let end_label = self.compiler.assembler.new_label();

        for arm in arms {
            let next_arm = self.compiler.assembler.new_label();
            if !arm.patterns.is_empty() {
                let matched = self.compiler.assembler.new_label();
                for pattern in &arm.patterns {
                    self.emit_copy_top_value(value_size)?;
                    emit_push_literal(
                        self.compiler,
                        &mut self.temp_bytes,
                        pattern,
                        &value.ty,
                        Some(arm.span),
                    )?;
                    self.emit_binary(BinaryOp::EqualEqual, &value.ty, &value.ty, Some(arm.span))?;
                    self.temp_bytes = self.temp_bytes.saturating_sub(4);
                    self.compiler.assembler.push_jump(NcsOpcode::Jnz, matched);
                }
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, next_arm);
                self.compiler.assembler.place_label(matched);
            }

            if let Some(guard) = &arm.guard {
                self.emit_expr(guard)?;
                self.emit_branch_zero(next_arm)?;
            }

            self.emit_pop_bytes(value_size)?;
            match &arm.body {
                crate::HirMatchArmBody::Expr(body) => self.emit_expr(body)?,
                crate::HirMatchArmBody::Block {
                    block,
                    tail,
                    ..
                } => {
                    self.emit_match_arm_block(block, tail.as_deref(), &expression.ty, result_size)?;
                }
            }
            if self.temp_bytes != base_temp_bytes + result_size {
                return Err(CodegenError::new(
                    Some(arm.span),
                    "match arm left an inconsistent value on the runtime stack",
                ));
            }
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, end_label);
            self.compiler.assembler.place_label(next_arm);
            self.temp_bytes = base_temp_bytes + value_size;
        }

        // Semantic checking makes matches exhaustive and prevents undeclared
        // enum values from entering source-level code. This unreachable
        // fallback only keeps the verifier's linear stack shape valid.
        self.emit_pop_bytes(value_size)?;
        if result_size > 0 {
            self.compiler.emit_stack_alloc(&expression.ty)?;
            self.temp_bytes += result_size;
        }
        self.compiler.assembler.place_label(end_label);
        self.temp_bytes = base_temp_bytes + result_size;
        Ok(())
    }

    pub(super) fn emit_checked_enum_conversion(
        &mut self,
        expression: &HirExpr,
        value: &HirExpr,
        fallback: &HirExpr,
        valid_values: &[Literal],
    ) -> Result<(), CodegenError> {
        let base_temp_bytes = self.temp_bytes;
        self.emit_expr(value)?;
        let value_size = size_of_type(&value.ty, &self.compiler.structs)?;
        let valid_label = self.compiler.assembler.new_label();
        let end_label = self.compiler.assembler.new_label();

        for valid_value in valid_values {
            self.emit_copy_top_value(value_size)?;
            emit_push_literal(
                self.compiler,
                &mut self.temp_bytes,
                valid_value,
                &value.ty,
                Some(expression.span),
            )?;
            self.emit_binary(
                BinaryOp::EqualEqual,
                &value.ty,
                &value.ty,
                Some(expression.span),
            )?;
            self.temp_bytes = self.temp_bytes.saturating_sub(4);
            self.compiler
                .assembler
                .push_jump(NcsOpcode::Jnz, valid_label);
        }

        self.emit_pop_bytes(value_size)?;
        self.emit_expr(fallback)?;
        self.compiler.assembler.push_jump(NcsOpcode::Jmp, end_label);

        self.compiler.assembler.place_label(valid_label);
        self.temp_bytes = base_temp_bytes + value_size;
        self.compiler.assembler.place_label(end_label);
        self.temp_bytes = base_temp_bytes + value_size;
        Ok(())
    }

    pub(super) fn emit_match_arm_block(
        &mut self,
        block: &HirBlock,
        tail: Option<&HirExpr>,
        result_type: &SemanticType,
        result_size: usize,
    ) -> Result<(), CodegenError> {
        self.scope_stack.push(FunctionScope {
            variable_debug: Vec::new(),
            locals:         Vec::new(),
            local_bytes:    0,
            temp_bytes:     self.temp_bytes,
        });
        for statement in &block.statements {
            self.emit_stmt(statement)?;
        }
        if let Some(tail) = tail {
            self.emit_expr(tail)?;
        } else if result_size > 0 {
            // A block that unconditionally returns/breaks/continues has no
            // runtime fallthrough, but code generation still needs a value
            // shape for the unreachable linear path after that jump.
            self.compiler.emit_stack_alloc(result_type)?;
            self.temp_bytes += result_size;
        }
        self.close_scope_preserving(result_size)
    }

    pub(super) fn close_scope_preserving(
        &mut self,
        result_size: usize,
    ) -> Result<(), CodegenError> {
        let Some(scope) = self.scope_stack.pop() else {
            return Ok(());
        };
        let expected_temps = scope.temp_bytes + result_size;
        if self.temp_bytes != expected_temps {
            return Err(CodegenError::new(
                None,
                "match block left unexpected expression temporaries",
            ));
        }
        let end = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(end);
        for index in scope.variable_debug {
            if let Some(variable) = self.compiler.variable_debug.get_mut(index) {
                variable.end = Some(end);
            }
        }
        if scope.local_bytes > 0 {
            let total = scope.local_bytes + result_size;
            let mut extra = Vec::with_capacity(6);
            extra.extend_from_slice(&usize_to_u16(total, "match block stack size")?.to_be_bytes());
            extra.extend_from_slice(
                &usize_to_u16(scope.local_bytes, "match result offset")?.to_be_bytes(),
            );
            extra.extend_from_slice(&usize_to_u16(result_size, "match result size")?.to_be_bytes());
            self.compiler.assembler.push(NcsInstruction {
                opcode: NcsOpcode::DeStruct,
                auxcode: NcsAuxCode::TypeVoid,
                extra,
            });
            self.layout.active_locals_size = self
                .layout
                .active_locals_size
                .saturating_sub(scope.local_bytes);
        }
        for local in scope.locals {
            self.layout.locals.remove(&local);
        }
        self.temp_bytes = expected_temps;
        Ok(())
    }

    pub(super) fn emit_call_with_match(
        &mut self,
        expr: &HirExpr,
        target: &HirCallTarget,
        arguments: &[HirExpr],
    ) -> Result<(), CodegenError> {
        let base_temp = self.temp_bytes;
        match target {
            HirCallTarget::Builtin(name) => {
                let (id, function) = self
                    .compiler
                    .builtin_functions
                    .get(name)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::new(Some(expr.span), format!("unknown builtin {name:?}"))
                    })?;
                for (index, parameter) in function.parameters.iter().enumerate().rev() {
                    if let Some(argument) = arguments.get(index) {
                        if matches!(parameter.ty, BuiltinType::Action) {
                            self.emit_action_parameter_with_match(argument)?;
                        } else {
                            self.emit_expr(argument)?;
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
                        let action =
                            lower_builtin_action_default_expr(self.compiler, default, expr.span)?;
                        self.emit_action_parameter_with_match(&action)?;
                        continue;
                    }
                    if matches!(parameter.ty, BuiltinType::Object)
                        && let BuiltinValue::ObjectId(value) = default
                    {
                        let object_id = if *value == 32_767 {
                            0x7f00_0000
                        } else {
                            *value
                        };
                        self.compiler.assembler.push(NcsInstruction {
                            opcode:  NcsOpcode::Constant,
                            auxcode: NcsAuxCode::TypeObject,
                            extra:   object_id.to_be_bytes().to_vec(),
                        });
                        self.temp_bytes += 4;
                        continue;
                    }
                    let literal = literal_from_builtin_value(default).ok_or_else(|| {
                        CodegenError::new(
                            Some(expr.span),
                            format!("unsupported builtin default value for {name:?}"),
                        )
                    })?;
                    let ty = semantic_type_from_builtin_type(&parameter.ty);
                    emit_push_literal(
                        self.compiler,
                        &mut self.temp_bytes,
                        &literal,
                        &ty,
                        Some(expr.span),
                    )?;
                }
                let return_size = size_of_type(&expr.ty, &self.compiler.structs)?;
                self.temp_bytes = base_temp + return_size;
                self.compiler.assembler.push(NcsInstruction {
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
                let function = self.compiler.functions.get(name).copied().ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown function {name:?}"))
                })?;
                if function.return_type != SemanticType::Void {
                    self.compiler.emit_stack_alloc(&function.return_type)?;
                    self.temp_bytes += size_of_type(&function.return_type, &self.compiler.structs)?;
                }
                for (index, parameter) in function.parameters.iter().enumerate().rev() {
                    if let Some(argument) = arguments.get(index) {
                        self.emit_expr(argument)?;
                    } else {
                        let default = parameter.default.as_ref().ok_or_else(|| {
                            CodegenError::new(
                                Some(expr.span),
                                format!("missing required parameter for function {name:?}"),
                            )
                        })?;
                        self.emit_expr(default)?;
                    }
                }
                let label = self
                    .compiler
                    .function_labels
                    .get(name)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::new(
                            Some(expr.span),
                            format!("missing function label for {name:?}"),
                        )
                    })?;
                self.compiler.assembler.push_jump(NcsOpcode::Jsr, label);
                let return_size = size_of_type(&function.return_type, &self.compiler.structs)?;
                self.temp_bytes = base_temp + return_size;
                Ok(())
            }
        }
    }

    pub(super) fn emit_action_parameter_with_match(
        &mut self,
        argument: &HirExpr,
    ) -> Result<(), CodegenError> {
        let stack_bytes = function_frame_bytes(&self.layout) + self.temp_bytes;
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::StoreState,
            auxcode: NcsAuxCode::TypeEngst0,
            extra:   store_state_extra(
                usize_to_u32(self.compiler.global_size, "global size")?,
                usize_to_u32(stack_bytes, "stack size")?,
            ),
        });
        let action_end = self.compiler.assembler.new_label();
        self.compiler
            .assembler
            .push_jump(NcsOpcode::Jmp, action_end);
        self.emit_expr(argument)?;
        self.compiler
            .assembler
            .push(simple_instruction(NcsOpcode::Ret));
        self.compiler.assembler.place_label(action_end);
        Ok(())
    }

    pub(super) fn emit_store_local(
        &mut self,
        local: HirLocalId,
        span: crate::Span,
    ) -> Result<(), CodegenError> {
        let layout = self.layout.locals.get(&local).ok_or_else(|| {
            CodegenError::new(Some(span), format!("unknown local slot {local:?}"))
        })?;
        let offset = usize_to_i32(layout.offset, "local slot offset")?
            - usize_to_i32(self.current_stack_bytes(), "current stack bytes")?;
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Assignment,
            auxcode: NcsAuxCode::TypeVoid,
            extra:   assignment_extra(offset, layout.size)?,
        });
        Ok(())
    }

    pub(super) fn emit_branch_zero(&mut self, target: LabelId) -> Result<(), CodegenError> {
        if self.temp_bytes < 4 {
            return Err(CodegenError::new(
                None,
                "branch expected an integer condition on the stack",
            ));
        }
        self.temp_bytes -= 4;
        self.compiler.assembler.push_jump(NcsOpcode::Jz, target);
        Ok(())
    }

    pub(super) fn emit_copy_top_value(&mut self, size: usize) -> Result<(), CodegenError> {
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::RunstackCopy,
            auxcode: NcsAuxCode::TypeVoid,
            extra:   assignment_extra(-usize_to_i32(size, "copy size")?, size)?,
        });
        self.temp_bytes += size;
        Ok(())
    }

    pub(super) fn emit_binary(
        &mut self,
        op: BinaryOp,
        left: &SemanticType,
        right: &SemanticType,
        span: Option<crate::Span>,
    ) -> Result<(), CodegenError> {
        let opcode = opcode_for_binary(op);
        let auxcode = aux_for_binary(left, right, self.compiler.langspec)?;
        let left_size = size_of_type(left, &self.compiler.structs)?;
        let right_size = size_of_type(right, &self.compiler.structs)?;
        let result_size = size_of_binary_result(op, left, right, &self.compiler.structs)?;
        if self.temp_bytes < left_size + right_size {
            return Err(CodegenError::new(
                span,
                "binary operation expected both operands on the stack",
            ));
        }
        self.temp_bytes -= left_size + right_size;
        self.temp_bytes += result_size;
        let extra = if auxcode == NcsAuxCode::TypeTypeStructStruct
            && matches!(opcode, NcsOpcode::Equal | NcsOpcode::NotEqual)
        {
            usize_to_u16(left_size, "struct equality size")?
                .to_be_bytes()
                .to_vec()
        } else {
            Vec::new()
        };
        self.compiler.assembler.push(NcsInstruction {
            opcode,
            auxcode,
            extra,
        });
        Ok(())
    }

    pub(super) fn emit_pop_type(&mut self, ty: &SemanticType) -> Result<(), CodegenError> {
        let size = size_of_type(ty, &self.compiler.structs)?;
        self.emit_pop_bytes(size)?;
        Ok(())
    }

    pub(super) fn emit_pop_bytes(&mut self, size: usize) -> Result<(), CodegenError> {
        if size > 0 {
            self.temp_bytes = self.temp_bytes.saturating_sub(size);
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(size, "stack pop size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        Ok(())
    }

    pub(super) fn emit_return_cleanup(&mut self) -> Result<(), CodegenError> {
        // A return statement removes live locals and expression temporaries,
        // then branches to the shared exit that removes parameters.
        let cleanup = self.layout.active_locals_size + self.temp_bytes;
        self.emit_cleanup_bytes(cleanup)
    }

    pub(super) fn emit_parameter_cleanup(&mut self) -> Result<(), CodegenError> {
        self.emit_cleanup_bytes(self.layout.parameter_size)
    }

    pub(super) fn emit_cleanup_bytes(&mut self, cleanup: usize) -> Result<(), CodegenError> {
        if cleanup > 0 {
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(cleanup, "stack cleanup size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        Ok(())
    }

    pub(super) fn current_stack_bytes(&self) -> usize {
        function_frame_bytes(&self.layout) + self.temp_bytes
    }
}
