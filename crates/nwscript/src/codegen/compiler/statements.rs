use super::*;

impl FunctionEmitter<'_, '_> {
    pub(super) fn emit_prologue(&mut self) -> Result<(), CodegenError> {
        Ok(())
    }

    pub(super) fn emit_block(&mut self, block: &HirBlock) -> Result<(), CodegenError> {
        self.scope_stack.push(FunctionScope {
            variable_debug: Vec::new(),
            locals:         Vec::new(),
            local_bytes:    0,
            temp_bytes:     self.temp_bytes,
        });
        for statement in &block.statements {
            self.emit_stmt(statement)?;
        }
        self.close_scope()?;
        Ok(())
    }

    pub(super) fn emit_stmt(&mut self, statement: &HirStmt) -> Result<(), CodegenError> {
        self.compiler.check_cancelled(Some(self.function.span))?;
        match statement {
            HirStmt::Block(block) => self.emit_block(block),
            HirStmt::Declare(statement) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(statement.span, start);
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                for declarator in &statement.declarators {
                    let local = self.local_info(declarator.local, statement.span)?.clone();
                    let size = size_of_type(&local.ty, &self.compiler.structs)?;
                    // A switch keeps its selector on the runtime stack while
                    // emitting the case body. Locals declared inside that
                    // body are therefore above both the normal frame and the
                    // live selector value.
                    let offset = self.current_stack_bytes();
                    self.compiler.emit_stack_alloc(&local.ty)?;
                    self.layout.locals.insert(
                        declarator.local,
                        ValueLayout {
                            offset,
                            size,
                        },
                    );
                    self.layout.active_locals_size += size;
                    let scope = self.current_scope(statement.span)?;
                    scope.locals.push(declarator.local);
                    scope.local_bytes += size;
                    let end_index = self.compiler.variable_debug.len();
                    self.compiler.variable_debug.push(VariableDebugInfo {
                        name: local.name.clone(),
                        ty: local.ty.clone(),
                        start,
                        end: None,
                        stack_loc: self.local_stack_loc(declarator.local, statement.span)?,
                    });
                    self.current_scope(statement.span)?
                        .variable_debug
                        .push(end_index);
                }
                for declarator in &statement.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        self.emit_expr(initializer)?;
                        self.emit_store_local(declarator.local, initializer.span)?;
                        self.emit_pop_type(&initializer.ty)?;
                    }
                }
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(statement.span, end);
                Ok(())
            }
            HirStmt::Expr(expr) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(expr.span, start);
                self.emit_expr(expr)?;
                self.emit_pop_type(&expr.ty)?;
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(expr.span, end);
                Ok(())
            }
            HirStmt::If(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let cond_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_start);
                self.compiler
                    .start_line_at(statement.condition.span, cond_start);
                let else_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.emit_expr(&statement.condition)?;
                self.emit_branch_zero(else_label)?;
                let cond_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_end);
                self.compiler
                    .end_line_at(statement.condition.span, cond_end);
                self.emit_stmt(&statement.then_branch)?;
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, end_label);
                self.compiler.assembler.place_label(else_label);
                if let Some(else_branch) = &statement.else_branch {
                    let choice_start = self.compiler.assembler.new_label();
                    self.compiler.assembler.place_label(choice_start);
                    self.compiler.start_line_at(statement.span, choice_start);
                    self.compiler
                        .assembler
                        .push(simple_instruction(NcsOpcode::NoOperation));
                    let choice_end = self.compiler.assembler.new_label();
                    self.compiler.assembler.place_label(choice_end);
                    self.compiler.end_line_at(statement.span, choice_end);
                    self.emit_stmt(else_branch)?;
                }
                self.compiler.assembler.place_label(end_label);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                Ok(())
            }
            HirStmt::Switch(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let result = self.emit_switch(statement);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                result
            }
            HirStmt::Return(statement) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(statement.span, start);
                if let Some(value) = &statement.value {
                    self.emit_expr(value)?;
                    let Some(retval) = &self.layout.return_layout else {
                        return Err(CodegenError::new(
                            Some(statement.span),
                            "return value in void function during code generation",
                        ));
                    };
                    let offset = usize_to_i32(retval.offset, "return slot offset")?
                        - usize_to_i32(self.current_stack_bytes(), "current stack bytes")?;
                    self.compiler.assembler.push(NcsInstruction {
                        opcode:  NcsOpcode::Assignment,
                        auxcode: NcsAuxCode::TypeVoid,
                        extra:   assignment_extra(offset, retval.size)?,
                    });
                }
                self.emit_return_cleanup()?;
                let exit = self
                    .compiler
                    .function_exit_labels
                    .get(&self.function.name)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::new(
                            Some(statement.span),
                            format!("missing function exit label for {:?}", self.function.name),
                        )
                    })?;
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, exit);
                // Native codegen still emits the expression-temporary pop
                // after the return jump. It is unreachable at runtime, but it
                // restores compile-time stack accounting for sibling control
                // flow and is required for byte-identical output.
                if let Some(value) = &statement.value {
                    self.emit_pop_type(&value.ty)?;
                }
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(statement.span, end);
                Ok(())
            }
            HirStmt::While(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let cond_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_label);
                let loop_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(loop_start);
                self.compiler.start_line_at(statement.span, loop_start);
                self.emit_expr(&statement.condition)?;
                self.emit_branch_zero(end_label)?;
                let loop_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(loop_end);
                self.compiler.end_line_at(statement.span, loop_end);
                self.break_targets.push(self.control_target(end_label));
                self.continue_targets.push(self.control_target(cond_label));
                self.emit_stmt(&statement.body)?;
                self.continue_targets.pop();
                self.break_targets.pop();
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, cond_label);
                self.compiler.assembler.place_label(end_label);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                Ok(())
            }
            HirStmt::DoWhile(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let body_label = self.compiler.assembler.new_label();
                let cond_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(body_label);
                self.break_targets.push(self.control_target(end_label));
                self.continue_targets.push(self.control_target(cond_label));
                self.emit_stmt(&statement.body)?;
                self.continue_targets.pop();
                self.break_targets.pop();
                self.compiler.assembler.place_label(cond_label);
                let continue_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(continue_start);
                self.compiler.start_line_at(statement.span, continue_start);
                self.emit_expr(&statement.condition)?;
                self.emit_branch_zero(end_label)?;
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, body_label);
                self.compiler.assembler.place_label(end_label);
                let continue_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(continue_end);
                self.compiler.end_line_at(statement.span, continue_end);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                Ok(())
            }
            HirStmt::For(statement) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(statement.span, start);
                if let Some(initializer) = &statement.initializer {
                    self.emit_expr(initializer)?;
                    self.emit_pop_type(&initializer.ty)?;
                }
                let cond_label = self.compiler.assembler.new_label();
                let update_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_label);
                if let Some(condition) = &statement.condition {
                    self.emit_expr(condition)?;
                    self.emit_branch_zero(end_label)?;
                }
                self.break_targets.push(self.control_target(end_label));
                self.continue_targets
                    .push(self.control_target(update_label));
                self.emit_stmt(&statement.body)?;
                self.continue_targets.pop();
                self.break_targets.pop();
                self.compiler.assembler.place_label(update_label);
                if let Some(update) = &statement.update {
                    self.emit_expr(update)?;
                    self.emit_pop_type(&update.ty)?;
                }
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, cond_label);
                self.compiler.assembler.place_label(end_label);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(statement.span, end);
                Ok(())
            }
            HirStmt::Case(_) | HirStmt::Default(_) => Err(CodegenError::new(
                None,
                "case/default labels must be lowered through emit_switch",
            )),
            HirStmt::Break(span) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(*span, start);
                let Some(target) = self.break_targets.last().copied() else {
                    return Err(CodegenError::new(
                        Some(*span),
                        "break used outside loop or switch",
                    ));
                };
                self.emit_control_cleanup(target)?;
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, target.label);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(*span, end);
                Ok(())
            }
            HirStmt::Continue(span) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(*span, start);
                let Some(target) = self.continue_targets.last().copied() else {
                    return Err(CodegenError::new(Some(*span), "continue used outside loop"));
                };
                self.emit_control_cleanup(target)?;
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, target.label);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(*span, end);
                Ok(())
            }
            HirStmt::Empty(span) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(*span, start);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(*span, end);
                Ok(())
            }
        }
    }

    pub(super) fn close_scope(&mut self) -> Result<(), CodegenError> {
        let Some(scope) = self.scope_stack.pop() else {
            return Ok(());
        };
        if self.temp_bytes > scope.temp_bytes {
            self.emit_pop_bytes(self.temp_bytes - scope.temp_bytes)?;
        }
        let end = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(end);
        for index in scope.variable_debug {
            if let Some(variable) = self.compiler.variable_debug.get_mut(index) {
                variable.end = Some(end);
            }
        }
        if scope.local_bytes > 0 {
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(scope.local_bytes, "scope cleanup size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
            self.layout.active_locals_size = self
                .layout
                .active_locals_size
                .saturating_sub(scope.local_bytes);
        }
        for local in scope.locals {
            self.layout.locals.remove(&local);
        }
        Ok(())
    }

    pub(super) fn current_scope(
        &mut self,
        span: crate::Span,
    ) -> Result<&mut FunctionScope, CodegenError> {
        self.scope_stack.last_mut().ok_or_else(|| {
            CodegenError::new(
                Some(span),
                "cannot emit a function-local declaration without an active scope",
            )
        })
    }

    pub(super) fn control_target(&self, label: LabelId) -> ControlTarget {
        ControlTarget {
            label,
            scope_depth: self.scope_stack.len(),
            temp_bytes: self.temp_bytes,
        }
    }

    pub(super) fn emit_control_cleanup(
        &mut self,
        target: ControlTarget,
    ) -> Result<(), CodegenError> {
        let local_bytes = self
            .scope_stack
            .get(target.scope_depth..)
            .ok_or_else(|| {
                CodegenError::new(
                    None,
                    "control-flow cleanup target references an invalid scope depth",
                )
            })?
            .iter()
            .map(|scope| scope.local_bytes)
            .sum::<usize>();
        let cleanup = local_bytes + self.temp_bytes.saturating_sub(target.temp_bytes);
        if cleanup > 0 {
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(cleanup, "control-flow cleanup size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        Ok(())
    }

    pub(super) fn local_info(
        &self,
        local_id: HirLocalId,
        span: crate::Span,
    ) -> Result<&crate::HirLocal, CodegenError> {
        self.function
            .locals
            .iter()
            .find(|local| local.id == local_id)
            .ok_or_else(|| CodegenError::new(Some(span), format!("unknown local {local_id:?}")))
    }

    pub(super) fn local_stack_loc(
        &self,
        local_id: HirLocalId,
        span: crate::Span,
    ) -> Result<u32, CodegenError> {
        let slot = self.layout.locals.get(&local_id).ok_or_else(|| {
            CodegenError::new(Some(span), format!("unknown local slot {local_id:?}"))
        })?;
        usize_to_u32(slot.offset, "local stack location")
    }

    pub(super) fn emit_switch(
        &mut self,
        statement: &crate::HirSwitchStmt,
    ) -> Result<(), CodegenError> {
        let HirStmt::Block(block) = statement.body.as_ref() else {
            return Err(CodegenError::new(
                Some(statement.span),
                "switch lowering requires a block body",
            ));
        };

        let switch_start = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(switch_start);
        self.compiler.start_line_at(statement.span, switch_start);
        self.emit_expr(&statement.condition)?;
        let switch_result_size = size_of_type(&statement.condition.ty, &self.compiler.structs)?;
        let body_end = self.compiler.assembler.new_label();
        let switch_eval_start = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(switch_eval_start);
        self.compiler.variable_debug.push(VariableDebugInfo {
            name:      "#switcheval".to_string(),
            ty:        statement.condition.ty.clone(),
            start:     switch_eval_start,
            end:       Some(body_end),
            stack_loc: usize_to_u32(
                self.current_stack_bytes()
                    .saturating_sub(switch_result_size),
                "switch stack location",
            )?,
        });

        let mut case_labels = Vec::new();
        let mut default_label = None;
        let mut case_index = 0usize;
        for stmt in &block.statements {
            match stmt {
                HirStmt::Case(case) => {
                    case_labels.push((
                        case_index,
                        evaluate_case_literal(case, &self.compiler.constant_env)?,
                        self.compiler.assembler.new_label(),
                    ));
                    case_index += 1;
                }
                HirStmt::Default(_) => {
                    let label = self.compiler.assembler.new_label();
                    default_label = Some((case_index, label));
                }
                _ => {}
            }
        }

        let next_test = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(next_test);
        for (_, value, label) in &case_labels {
            self.emit_copy_top_value(switch_result_size)?;
            emit_push_literal(
                self.compiler,
                &mut self.temp_bytes,
                value,
                &statement.condition.ty,
                Some(statement.span),
            )?;
            self.emit_binary(
                BinaryOp::EqualEqual,
                &statement.condition.ty,
                &statement.condition.ty,
                Some(statement.span),
            )?;
            self.temp_bytes = self.temp_bytes.saturating_sub(4);
            self.compiler.assembler.push_jump(NcsOpcode::Jnz, *label);
        }
        if let Some((_, label)) = default_label {
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, label);
        } else {
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, body_end);
        }

        let break_target = self.control_target(body_end);
        self.break_targets.push(break_target);
        self.scope_stack.push(FunctionScope {
            variable_debug: Vec::new(),
            locals:         Vec::new(),
            local_bytes:    0,
            temp_bytes:     self.temp_bytes,
        });
        let mut seen_cases = 0usize;
        for stmt in &block.statements {
            match stmt {
                HirStmt::Case(_) => {
                    let Some((_, _, label)) = case_labels.get(seen_cases) else {
                        return Err(CodegenError::new(
                            Some(statement.span),
                            "switch case label index out of bounds",
                        ));
                    };
                    seen_cases += 1;
                    self.compiler.assembler.place_label(*label);
                }
                HirStmt::Default(span) => {
                    let Some((_, label)) = default_label else {
                        return Err(CodegenError::new(Some(*span), "missing default label"));
                    };
                    self.compiler.assembler.place_label(label);
                }
                other => self.emit_stmt(other)?,
            }
        }
        self.break_targets.pop();
        self.close_scope()?;
        self.compiler.assembler.place_label(body_end);
        self.emit_pop_bytes(switch_result_size)?;
        Ok(())
    }
}
