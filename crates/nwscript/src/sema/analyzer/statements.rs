use super::*;

impl Analyzer<'_> {
    pub(super) fn analyze_function_bodies(&self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Function(function) = item else {
                continue;
            };
            let Some(body) = &function.body else {
                continue;
            };

            let info = self.functions.get(&function.name).ok_or_else(|| {
                SemanticError::new(
                    CompilerErrorCode::UnknownStateInCompiler,
                    function.span,
                    format!("function {:?} missing from semantic table", function.name),
                )
            })?;
            let implementation_signature = self.resolve_function_signature(function)?;
            let mut scopes = vec![BTreeMap::new(), BTreeMap::new()];
            for (signature_parameter, source_parameter) in
                info.signature.parameters.iter().zip(&function.parameters)
            {
                let (parameter_scope, _) = scopes.split_at_mut(1);
                insert_scope_binding(
                    parameter_scope,
                    &source_parameter.name,
                    signature_parameter.ty.clone(),
                    false,
                    source_parameter.span,
                )?;
            }

            let mut context = AnalysisContext::default();
            self.analyze_block(
                body,
                &mut scopes,
                &implementation_signature.return_type,
                true,
                &mut context,
            )?;

            if implementation_signature.return_type != SemanticType::Void
                && !statement_guarantees_return(&Stmt::Block(body.clone()))
            {
                return Err(SemanticError::new(
                    CompilerErrorCode::NotAllControlPathsReturnAValue,
                    function.span,
                    format!(
                        "function {:?} does not return a value on all control paths",
                        function.name
                    ),
                ));
            }
        }

        Ok(())
    }

    pub(super) fn analyze_block(
        &self,
        block: &BlockStmt,
        scopes: &mut Vec<BTreeMap<String, ScopeBinding>>,
        return_type: &SemanticType,
        is_function_body: bool,
        context: &mut AnalysisContext,
    ) -> Result<(), SemanticError> {
        if !is_function_body {
            scopes.push(BTreeMap::new());
            context.enter_scope();
        }

        for statement in &block.statements {
            self.check_cancelled(block.span)?;
            self.analyze_stmt(statement, scopes, return_type, context)?;
        }

        if !is_function_body {
            context.exit_scope();
            scopes.pop();
        }

        Ok(())
    }

    pub(super) fn analyze_stmt(
        &self,
        statement: &Stmt,
        scopes: &mut Vec<BTreeMap<String, ScopeBinding>>,
        return_type: &SemanticType,
        context: &mut AnalysisContext,
    ) -> Result<(), SemanticError> {
        match statement {
            Stmt::Block(block) => self.analyze_block(block, scopes, return_type, false, context),
            Stmt::Declaration(declaration) => {
                if declaration.ty.is_const {
                    return Err(SemanticError::new(
                        CompilerErrorCode::ConstKeywordCannotBeUsedOnNonGlobalVariables,
                        declaration.span,
                        "const cannot be used on non-global variables",
                    ));
                }

                let ty = self.resolve_type(&declaration.ty)?;
                for declarator in &declaration.declarators {
                    if current_scope_contains(scopes, &declarator.name) {
                        return Err(SemanticError::new(
                            CompilerErrorCode::VariableAlreadyUsedWithinScope,
                            declarator.span,
                            format!(
                                "variable {:?} was already declared in this scope",
                                declarator.name
                            ),
                        ));
                    }

                    let Some(scope) = scopes.last_mut() else {
                        return Err(SemanticError::new(
                            CompilerErrorCode::UnknownStateInCompiler,
                            declarator.span,
                            "scope stack must be non-empty",
                        ));
                    };
                    scope.insert(
                        declarator.name.clone(),
                        ScopeBinding {
                            ty:       ty.clone(),
                            is_const: false,
                        },
                    );

                    if let Some(initializer) = &declarator.initializer {
                        let initializer_type = self
                            .analyze_expr_with_context(initializer, scopes, return_type, context)?
                            .ty;
                        if !types_compatible(&ty, &initializer_type) {
                            return Err(SemanticError::new(
                                type_mismatch_code(&ty, &initializer_type),
                                initializer.span,
                                format!(
                                    "initializer for {:?} has type {:?}, expected {:?}",
                                    declarator.name, initializer_type, ty
                                ),
                            ));
                        }
                    }
                }
                context.record_declaration();
                Ok(())
            }
            Stmt::Expression(statement) => {
                self.analyze_expr_with_context(&statement.expr, scopes, return_type, context)?;
                Ok(())
            }
            Stmt::If(statement) => {
                let condition = self.analyze_expr_with_context(
                    &statement.condition,
                    scopes,
                    return_type,
                    context,
                )?;
                if condition.ty != SemanticType::Int {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonIntegerExpressionWhereIntegerRequired,
                        statement.condition.span,
                        "if condition must evaluate to int",
                    ));
                }
                self.analyze_stmt(&statement.then_branch, scopes, return_type, context)?;
                if let Some(branch) = &statement.else_branch {
                    self.analyze_stmt(branch, scopes, return_type, context)?;
                }
                Ok(())
            }
            Stmt::Switch(statement) => {
                let condition = self.analyze_expr_with_context(
                    &statement.condition,
                    scopes,
                    return_type,
                    context,
                )?;
                if condition.ty != SemanticType::Int
                    && !matches!(condition.ty, SemanticType::Enum { .. })
                {
                    return Err(SemanticError::new(
                        CompilerErrorCode::SwitchMustEvaluateToAnInteger,
                        statement.condition.span,
                        "switch condition must evaluate to int or a strong enum",
                    ));
                }
                context.switch_stack.push(SwitchContext {
                    case_values:       BTreeSet::new(),
                    has_default:       false,
                    scope_decl_counts: vec![0],
                    condition_type:    condition.ty,
                });
                let result = self.analyze_stmt(&statement.body, scopes, return_type, context);
                context.switch_stack.pop();
                result
            }
            Stmt::Return(statement) => match (&statement.value, return_type) {
                (None, SemanticType::Void) => Ok(()),
                (Some(value), SemanticType::Void) => Err(SemanticError::new(
                    CompilerErrorCode::ReturnTypeAndFunctionTypeMismatched,
                    value.span,
                    "void functions cannot return a value",
                )),
                (None, _) => Err(SemanticError::new(
                    CompilerErrorCode::ReturnTypeAndFunctionTypeMismatched,
                    statement.span,
                    "non-void functions must return a value",
                )),
                (Some(value), expected) => {
                    let actual = self
                        .analyze_expr_with_context(value, scopes, return_type, context)?
                        .ty;
                    if !types_compatible(expected, &actual) {
                        return Err(SemanticError::new(
                            CompilerErrorCode::ReturnTypeAndFunctionTypeMismatched,
                            value.span,
                            format!("return expression has type {actual:?}, expected {expected:?}"),
                        ));
                    }
                    Ok(())
                }
            },
            Stmt::While(statement) => {
                let condition = self.analyze_expr_with_context(
                    &statement.condition,
                    scopes,
                    return_type,
                    context,
                )?;
                if condition.ty != SemanticType::Int {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonIntegerExpressionWhereIntegerRequired,
                        statement.condition.span,
                        "while condition must evaluate to int",
                    ));
                }
                context.loop_depth += 1;
                let result = self.analyze_stmt(&statement.body, scopes, return_type, context);
                context.loop_depth = context.loop_depth.saturating_sub(1);
                result
            }
            Stmt::DoWhile(statement) => {
                context.loop_depth += 1;
                let body_result = self.analyze_stmt(&statement.body, scopes, return_type, context);
                context.loop_depth = context.loop_depth.saturating_sub(1);
                body_result?;
                let condition = self.analyze_expr_with_context(
                    &statement.condition,
                    scopes,
                    return_type,
                    context,
                )?;
                if condition.ty != SemanticType::Int {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonIntegerExpressionWhereIntegerRequired,
                        statement.condition.span,
                        "do-while condition must evaluate to int",
                    ));
                }
                Ok(())
            }
            Stmt::For(statement) => {
                if let Some(initializer) = &statement.initializer {
                    self.analyze_expr_with_context(initializer, scopes, return_type, context)?;
                }
                if let Some(condition) = &statement.condition {
                    let resolved =
                        self.analyze_expr_with_context(condition, scopes, return_type, context)?;
                    if resolved.ty != SemanticType::Int {
                        return Err(SemanticError::new(
                            CompilerErrorCode::NonIntegerExpressionWhereIntegerRequired,
                            condition.span,
                            "for condition must evaluate to int",
                        ));
                    }
                }
                if let Some(update) = &statement.update {
                    self.analyze_expr_with_context(update, scopes, return_type, context)?;
                }
                context.loop_depth += 1;
                let result = self.analyze_stmt(&statement.body, scopes, return_type, context);
                context.loop_depth = context.loop_depth.saturating_sub(1);
                result
            }
            Stmt::Case(statement) => {
                let case_type = self
                    .analyze_expr_with_context(&statement.value, scopes, return_type, context)?
                    .ty;
                let value = self
                    .evaluate_switch_case_value(&statement.value)
                    .ok_or_else(|| {
                        SemanticError::new(
                            CompilerErrorCode::CaseParameterNotAConstantInteger,
                            statement.value.span,
                            "case expression must be a constant integer or string",
                        )
                    })?;
                let Some(current_switch) = context.current_switch_mut() else {
                    return Err(SemanticError::new(
                        CompilerErrorCode::UnknownStateInCompiler,
                        statement.span,
                        "case labels must appear within a switch statement",
                    ));
                };
                if (matches!(current_switch.condition_type, SemanticType::Enum { .. })
                    || matches!(case_type, SemanticType::Enum { .. }))
                    && current_switch.condition_type != case_type
                {
                    return Err(SemanticError::new(
                        CompilerErrorCode::InvalidEnumOperation,
                        statement.value.span,
                        format!(
                            "switch condition has type {:?}, but case has type {case_type:?}",
                            current_switch.condition_type
                        ),
                    ));
                }
                if current_switch.has_live_declarations() {
                    return Err(SemanticError::new(
                        CompilerErrorCode::JumpingOverDeclarationStatementsCaseDisallowed,
                        statement.span,
                        "case label would jump over active declarations",
                    ));
                }
                if !current_switch.case_values.insert(value) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::MultipleCaseConstantStatementsWithinSwitch,
                        statement.span,
                        format!("case value {value:?} was used more than once in this switch"),
                    ));
                }
                Ok(())
            }
            Stmt::Default(statement) => {
                let Some(current_switch) = context.current_switch_mut() else {
                    return Err(SemanticError::new(
                        CompilerErrorCode::UnknownStateInCompiler,
                        statement.span,
                        "default labels must appear within a switch statement",
                    ));
                };
                if current_switch.has_live_declarations() {
                    return Err(SemanticError::new(
                        CompilerErrorCode::JumpingOverDeclarationStatementsDefaultDisallowed,
                        statement.span,
                        "default label would jump over active declarations",
                    ));
                }
                if current_switch.has_default {
                    return Err(SemanticError::new(
                        CompilerErrorCode::MultipleDefaultStatementsWithinSwitch,
                        statement.span,
                        "default label appeared more than once in this switch",
                    ));
                }
                current_switch.has_default = true;
                Ok(())
            }
            Stmt::Break(span) => {
                if context.loop_depth == 0 && context.switch_stack.is_empty() {
                    return Err(SemanticError::new(
                        CompilerErrorCode::BreakOutsideOfLoopOrCaseStatement,
                        span.span,
                        "break must appear within a loop or switch statement",
                    ));
                }
                Ok(())
            }
            Stmt::Continue(span) => {
                if context.loop_depth == 0 {
                    return Err(SemanticError::new(
                        CompilerErrorCode::BreakOutsideOfLoopOrCaseStatement,
                        span.span,
                        "continue must appear within a loop statement",
                    ));
                }
                Ok(())
            }
            Stmt::Empty(_) => Ok(()),
            Stmt::StaticAssert(assertion) => self.validate_static_assert(assertion),
        }
    }
}
