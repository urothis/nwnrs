use super::*;

impl Analyzer<'_> {
    pub(super) fn analyze_expr(
        &self,
        expr: &Expr,
        scopes: &mut Vec<BTreeMap<String, ScopeBinding>>,
    ) -> Result<ResolvedExpr, SemanticError> {
        let mut context = AnalysisContext::default();
        self.analyze_expr_with_context(expr, scopes, &SemanticType::Void, &mut context)
    }

    pub(super) fn analyze_expr_with_context(
        &self,
        expr: &Expr,
        scopes: &mut Vec<BTreeMap<String, ScopeBinding>>,
        return_type: &SemanticType,
        context: &mut AnalysisContext,
    ) -> Result<ResolvedExpr, SemanticError> {
        self.check_cancelled(expr.span)?;
        match &expr.kind {
            ExprKind::Literal(literal) => Ok(ResolvedExpr {
                ty:        semantic_type_from_literal(literal),
                is_lvalue: false,
                is_const:  !matches!(literal, Literal::Magic(_)),
            }),
            ExprKind::Identifier(name) => {
                let binding = self.lookup_value(name, scopes).ok_or_else(|| {
                    SemanticError::new(
                        CompilerErrorCode::UndefinedIdentifier,
                        expr.span,
                        format!("undefined identifier {name:?}"),
                    )
                })?;
                Ok(ResolvedExpr {
                    ty:        binding.ty(),
                    is_lvalue: matches!(binding, ValueBinding::Variable { .. }),
                    is_const:  binding.is_const(),
                })
            }
            ExprKind::ScopedIdentifier {
                scope,
                name,
            } => {
                let enum_name = self.canonical_enum_name(scope).ok_or_else(|| {
                    SemanticError::new(
                        CompilerErrorCode::InvalidEnumOperation,
                        expr.span,
                        format!("undefined enum type {scope:?}"),
                    )
                })?;
                let declaration = self.enums.get(enum_name).ok_or_else(|| {
                    SemanticError::new(
                        CompilerErrorCode::InvalidEnumOperation,
                        expr.span,
                        format!("undefined enum type {scope:?}"),
                    )
                })?;
                declaration
                    .variants
                    .iter()
                    .find(|variant| variant.name == *name)
                    .ok_or_else(|| {
                        SemanticError::new(
                            CompilerErrorCode::InvalidEnumOperation,
                            expr.span,
                            format!("enum {scope:?} has no variant {name:?}"),
                        )
                    })?;
                Ok(ResolvedExpr {
                    ty:        SemanticType::Enum {
                        name:    enum_name.to_string(),
                        backing: declaration.backing,
                    },
                    is_lvalue: false,
                    is_const:  true,
                })
            }
            ExprKind::Match(expression) => {
                let matched = self.analyze_expr_with_context(
                    &expression.value,
                    scopes,
                    return_type,
                    context,
                )?;
                let SemanticType::Enum {
                    name: enum_name, ..
                } = &matched.ty
                else {
                    return Err(SemanticError::new(
                        CompilerErrorCode::InvalidMatch,
                        expression.value.span,
                        "match value must have a strong enum type",
                    ));
                };
                let declaration = self.enums.get(enum_name).ok_or_else(|| {
                    SemanticError::new(
                        CompilerErrorCode::InvalidMatch,
                        expression.value.span,
                        format!("missing enum declaration {enum_name:?}"),
                    )
                })?;
                if expression.arms.is_empty() {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonExhaustiveMatch,
                        expr.span,
                        "match must contain at least one arm",
                    ));
                }

                let mut covered = BTreeSet::new();
                let mut wildcard_covered = false;
                let mut result_type = None;
                for arm in &expression.arms {
                    if wildcard_covered || covered.len() == declaration.variants.len() {
                        return Err(SemanticError::new(
                            CompilerErrorCode::UnreachableMatchArm,
                            arm.span,
                            "match arm is unreachable after an unguarded wildcard",
                        ));
                    }
                    let mut arm_names = BTreeSet::new();
                    let has_wildcard = arm
                        .patterns
                        .iter()
                        .any(|pattern| matches!(pattern, crate::MatchPattern::Wildcard { .. }));
                    if has_wildcard && arm.patterns.len() != 1 {
                        return Err(SemanticError::new(
                            CompilerErrorCode::UnreachableMatchArm,
                            arm.span,
                            "wildcard must be the only pattern in its match arm",
                        ));
                    }
                    for pattern in &arm.patterns {
                        let crate::MatchPattern::Variant {
                            span,
                            scope,
                            name,
                        } = pattern
                        else {
                            continue;
                        };
                        if self.canonical_enum_name(scope) != Some(enum_name.as_str()) {
                            return Err(SemanticError::new(
                                CompilerErrorCode::InvalidMatch,
                                *span,
                                format!(
                                    "match for {enum_name:?} cannot use variant from {scope:?}"
                                ),
                            ));
                        }
                        if !declaration
                            .variants
                            .iter()
                            .any(|variant| variant.name == *name)
                        {
                            return Err(SemanticError::new(
                                CompilerErrorCode::InvalidMatch,
                                *span,
                                format!("enum {enum_name:?} has no variant {name:?}"),
                            ));
                        }
                        if !arm_names.insert(name.clone()) || covered.contains(name) {
                            return Err(SemanticError::new(
                                CompilerErrorCode::UnreachableMatchArm,
                                *span,
                                format!("match pattern {enum_name}::{name} is unreachable"),
                            ));
                        }
                    }

                    let guard_is_always_true = if let Some(guard) = &arm.guard {
                        let guard_type = self
                            .analyze_expr_with_context(guard, scopes, return_type, context)?
                            .ty;
                        if guard_type != SemanticType::Int {
                            return Err(SemanticError::new(
                                CompilerErrorCode::InvalidMatch,
                                guard.span,
                                "match guard must evaluate to int",
                            ));
                        }
                        match self.evaluate_constant_expr(guard) {
                            Some(ConstantValue::Int(0)) => {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::UnreachableMatchArm,
                                    arm.span,
                                    "match arm has a guard that is always false",
                                ));
                            }
                            Some(ConstantValue::Int(_)) => true,
                            _ => false,
                        }
                    } else {
                        true
                    };

                    let arm_type = match &arm.body {
                        crate::MatchArmBody::Expr(body) => Some(
                            self.analyze_expr_with_context(body, scopes, return_type, context)?
                                .ty,
                        ),
                        crate::MatchArmBody::Block(block) => {
                            scopes.push(BTreeMap::new());
                            context.enter_scope();
                            let block_result = block
                                .statements
                                .iter()
                                .try_for_each(|statement| {
                                    self.analyze_stmt(statement, scopes, return_type, context)
                                })
                                .and_then(|()| {
                                    block.tail.as_ref().map_or_else(
                                        || {
                                            Ok((!match_block_guarantees_exit(block))
                                                .then_some(SemanticType::Void))
                                        },
                                        |tail| {
                                            self.analyze_expr_with_context(
                                                tail,
                                                scopes,
                                                return_type,
                                                context,
                                            )
                                            .map(|resolved| Some(resolved.ty))
                                        },
                                    )
                                });
                            context.exit_scope();
                            scopes.pop();
                            block_result?
                        }
                    };
                    if let Some(arm_type) = arm_type {
                        if let Some(expected) = &result_type {
                            if !types_compatible(expected, &arm_type) {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::InvalidMatch,
                                    arm.span,
                                    format!(
                                        "match arm has type {arm_type:?}, expected {expected:?}"
                                    ),
                                ));
                            }
                        } else {
                            result_type = Some(arm_type);
                        }
                    }

                    if guard_is_always_true {
                        if has_wildcard {
                            wildcard_covered = true;
                        } else {
                            covered.extend(arm_names);
                        }
                    }
                }

                if !wildcard_covered {
                    let missing = declaration
                        .variants
                        .iter()
                        .filter(|variant| !covered.contains(&variant.name))
                        .map(|variant| format!("{enum_name}::{}", variant.name))
                        .collect::<Vec<_>>();
                    if !missing.is_empty() {
                        return Err(SemanticError::new(
                            CompilerErrorCode::NonExhaustiveMatch,
                            expr.span,
                            format!("non-exhaustive match; missing {}", missing.join(", ")),
                        ));
                    }
                }

                Ok(ResolvedExpr {
                    ty:        result_type.unwrap_or(SemanticType::Void),
                    is_lvalue: false,
                    is_const:  false,
                })
            }
            ExprKind::Call {
                callee,
                arguments,
            } => {
                let ExprKind::Identifier(name) = &callee.kind else {
                    return Err(SemanticError::new(
                        CompilerErrorCode::UndefinedIdentifier,
                        callee.span,
                        "only direct identifier calls are supported",
                    ));
                };

                let conversion_target = match name.as_str() {
                    "int" => Some(SemanticType::Int),
                    "string" => Some(SemanticType::String),
                    _ => self.aliases.get(name).cloned().or_else(|| {
                        self.enums.get(name).map(|declaration| SemanticType::Enum {
                            name:    name.clone(),
                            backing: declaration.backing,
                        })
                    }),
                };
                if let Some(target) = conversion_target {
                    let Some(argument) = arguments.first() else {
                        return Err(SemanticError::new(
                            CompilerErrorCode::InvalidEnumOperation,
                            expr.span,
                            format!("explicit conversion {name}(...) requires an argument"),
                        ));
                    };
                    let resolved =
                        self.analyze_expr_with_context(argument, scopes, return_type, context)?;
                    match &target {
                        SemanticType::Enum {
                            name: enum_name,
                            backing,
                        } => {
                            let expected_backing = match backing {
                                EnumBackingType::Int => SemanticType::Int,
                                EnumBackingType::String => SemanticType::String,
                            };
                            if resolved.ty != expected_backing {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::InvalidEnumOperation,
                                    argument.span,
                                    format!(
                                        "cannot explicitly convert {:?} to {target:?}",
                                        resolved.ty
                                    ),
                                ));
                            }
                            if arguments.len() == 1 {
                                let value =
                                    self.evaluate_constant_expr(argument).ok_or_else(|| {
                                        SemanticError::new(
                                            CompilerErrorCode::InvalidEnumOperation,
                                            argument.span,
                                            format!(
                                                "dynamic conversion to {enum_name} requires an \
                                                 explicit fallback: {name}(value, \
                                                 {enum_name}::Variant)"
                                            ),
                                        )
                                    })?;
                                let declaration = self.enums.get(enum_name).ok_or_else(|| {
                                    SemanticError::new(
                                        CompilerErrorCode::InvalidEnumOperation,
                                        expr.span,
                                        format!("missing enum declaration {enum_name:?}"),
                                    )
                                })?;
                                let valid = declaration.variants.iter().any(|variant| {
                                    matches!(
                                        (&value, &variant.value),
                                        (ConstantValue::Int(left), SemanticEnumValue::Int(right)) if left == right
                                    ) || matches!(
                                        (&value, &variant.value),
                                        (ConstantValue::String(left), SemanticEnumValue::String(right)) if left == right
                                    )
                                });
                                if !valid {
                                    return Err(SemanticError::new(
                                        CompilerErrorCode::InvalidEnumOperation,
                                        argument.span,
                                        format!(
                                            "constant conversion to {enum_name} does not name a \
                                             declared variant"
                                        ),
                                    ));
                                }
                                return Ok(ResolvedExpr {
                                    ty:        target,
                                    is_lvalue: false,
                                    is_const:  true,
                                });
                            }
                            if arguments.len() != 2 {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::InvalidEnumOperation,
                                    expr.span,
                                    format!(
                                        "checked conversion {name}(...) requires a value and one \
                                         fallback variant"
                                    ),
                                ));
                            }
                            let fallback = arguments.get(1).ok_or_else(|| {
                                SemanticError::new(
                                    CompilerErrorCode::InvalidEnumOperation,
                                    expr.span,
                                    format!(
                                        "checked conversion {name}(...) requires a fallback \
                                         variant"
                                    ),
                                )
                            })?;
                            let fallback = self.analyze_expr_with_context(
                                fallback,
                                scopes,
                                return_type,
                                context,
                            )?;
                            if fallback.ty != target {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::InvalidEnumOperation,
                                    arguments.get(1).map_or(expr.span, |fallback| fallback.span),
                                    format!(
                                        "checked conversion fallback has type {:?}, expected \
                                         {target:?}",
                                        fallback.ty
                                    ),
                                ));
                            }
                            return Ok(ResolvedExpr {
                                ty:        target,
                                is_lvalue: false,
                                is_const:  resolved.is_const && fallback.is_const,
                            });
                        }
                        SemanticType::Int | SemanticType::String => {
                            if arguments.len() != 1 {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::InvalidEnumOperation,
                                    expr.span,
                                    format!(
                                        "explicit conversion {name}(...) requires one argument"
                                    ),
                                ));
                            }
                            let valid = matches!(
                                (&target, &resolved.ty),
                                (
                                    SemanticType::Int,
                                    SemanticType::Enum {
                                        backing: EnumBackingType::Int,
                                        ..
                                    },
                                ) | (
                                    SemanticType::String,
                                    SemanticType::Enum {
                                        backing: EnumBackingType::String,
                                        ..
                                    },
                                )
                            );
                            if !valid {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::InvalidEnumOperation,
                                    argument.span,
                                    format!(
                                        "cannot explicitly convert {:?} to {target:?}",
                                        resolved.ty
                                    ),
                                ));
                            }
                            return Ok(ResolvedExpr {
                                ty:        target,
                                is_lvalue: false,
                                is_const:  resolved.is_const,
                            });
                        }
                        _ => {}
                    }
                }

                let function = self.functions.get(name).ok_or_else(|| {
                    SemanticError::new(
                        CompilerErrorCode::UndefinedIdentifier,
                        callee.span,
                        format!("undefined function {name:?}"),
                    )
                })?;

                if arguments.len() > function.signature.parameters.len() {
                    return Err(SemanticError::new(
                        CompilerErrorCode::DeclarationDoesNotMatchParameters,
                        expr.span,
                        format!(
                            "call to {:?} passed too many parameters: {} > {}",
                            name,
                            arguments.len(),
                            function.signature.parameters.len()
                        ),
                    ));
                }

                let non_optional = function
                    .signature
                    .parameters
                    .iter()
                    .filter(|parameter| !parameter.is_optional)
                    .count();
                if arguments.len() < non_optional {
                    return Err(SemanticError::new(
                        CompilerErrorCode::DeclarationDoesNotMatchParameters,
                        expr.span,
                        format!(
                            "call to {:?} did not supply enough parameters: {} < {}",
                            name,
                            arguments.len(),
                            non_optional
                        ),
                    ));
                }

                for (argument, parameter) in arguments.iter().zip(&function.signature.parameters) {
                    let resolved =
                        self.analyze_expr_with_context(argument, scopes, return_type, context)?;
                    let is_action_argument = matches!(
                        (&parameter.ty, &argument.kind),
                        (
                            SemanticType::Action,
                            ExprKind::Call {
                                callee:    _,
                                arguments: _,
                            }
                        )
                    ) && resolved.ty == SemanticType::Void;
                    if !is_action_argument && !types_compatible(&parameter.ty, &resolved.ty) {
                        let code = if matches!(parameter.ty, SemanticType::Enum { .. })
                            || matches!(resolved.ty, SemanticType::Enum { .. })
                        {
                            CompilerErrorCode::InvalidEnumOperation
                        } else {
                            CompilerErrorCode::DeclarationDoesNotMatchParameters
                        };
                        return Err(SemanticError::new(
                            code,
                            argument.span,
                            format!(
                                "parameter {:?} expects {:?}, got {:?}",
                                parameter.name, parameter.ty, resolved.ty
                            ),
                        ));
                    }
                }

                Ok(ResolvedExpr {
                    ty:        function.signature.return_type.clone(),
                    is_lvalue: false,
                    is_const:  false,
                })
            }
            ExprKind::FieldAccess {
                base,
                field,
            } => {
                let resolved_base =
                    self.analyze_expr_with_context(base, scopes, return_type, context)?;
                let field_type = match &resolved_base.ty {
                    SemanticType::Vector => match field.as_str() {
                        "x" | "y" | "z" => Ok(SemanticType::Float),
                        _ => Err(SemanticError::new(
                            CompilerErrorCode::UndefinedFieldInStructure,
                            expr.span,
                            format!("field {field:?} does not exist on vector"),
                        )),
                    },
                    SemanticType::Struct(name) => self
                        .structs
                        .get(name)
                        .and_then(|structure| {
                            structure
                                .fields
                                .iter()
                                .find(|candidate| candidate.name == *field)
                                .map(|candidate| candidate.ty.clone())
                        })
                        .ok_or_else(|| {
                            SemanticError::new(
                                CompilerErrorCode::UndefinedFieldInStructure,
                                expr.span,
                                format!("field {field:?} does not exist on structure {name:?}"),
                            )
                        }),
                    _ => Err(SemanticError::new(
                        CompilerErrorCode::LeftOfStructurePartNotStructure,
                        base.span,
                        "left side of field access must be a structure",
                    )),
                }?;

                Ok(ResolvedExpr {
                    ty:        field_type,
                    is_lvalue: resolved_base.is_lvalue,
                    is_const:  resolved_base.is_const,
                })
            }
            ExprKind::Unary {
                op,
                expr: inner,
            } => {
                let resolved =
                    self.analyze_expr_with_context(inner, scopes, return_type, context)?;
                if matches!(resolved.ty, SemanticType::Enum { .. }) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::InvalidEnumOperation,
                        expr.span,
                        format!(
                            "unary operation {op:?} is invalid for strong enum type {:?}; convert \
                             explicitly to the backing type first",
                            resolved.ty
                        ),
                    ));
                }
                match op {
                    UnaryOp::Negate => match resolved.ty {
                        SemanticType::Int | SemanticType::Float => Ok(ResolvedExpr {
                            ty:        resolved.ty,
                            is_lvalue: false,
                            is_const:  resolved.is_const,
                        }),
                        _ => Err(SemanticError::new(
                            CompilerErrorCode::ArithmeticOperationHasInvalidOperands,
                            expr.span,
                            "negation requires an int or float operand",
                        )),
                    },
                    UnaryOp::OnesComplement => {
                        if resolved.ty != SemanticType::Int {
                            return Err(SemanticError::new(
                                CompilerErrorCode::ArithmeticOperationHasInvalidOperands,
                                expr.span,
                                "ones-complement requires an int operand",
                            ));
                        }
                        Ok(ResolvedExpr {
                            ty:        SemanticType::Int,
                            is_lvalue: false,
                            is_const:  resolved.is_const,
                        })
                    }
                    UnaryOp::BooleanNot => {
                        if resolved.ty != SemanticType::Int {
                            return Err(SemanticError::new(
                                CompilerErrorCode::LogicalOperationHasInvalidOperands,
                                expr.span,
                                "boolean-not requires an int operand",
                            ));
                        }
                        Ok(ResolvedExpr {
                            ty:        SemanticType::Int,
                            is_lvalue: false,
                            is_const:  resolved.is_const,
                        })
                    }
                    UnaryOp::PreIncrement
                    | UnaryOp::PreDecrement
                    | UnaryOp::PostIncrement
                    | UnaryOp::PostDecrement => {
                        if resolved.ty != SemanticType::Int || !resolved.is_lvalue {
                            return Err(SemanticError::new(
                                CompilerErrorCode::OperandMustBeAnIntegerLValue,
                                expr.span,
                                "increment and decrement require an int lvalue",
                            ));
                        }
                        Ok(ResolvedExpr {
                            ty:        SemanticType::Int,
                            is_lvalue: false,
                            is_const:  false,
                        })
                    }
                }
            }
            ExprKind::Binary {
                op,
                left,
                right,
            } => {
                let left = self.analyze_expr_with_context(left, scopes, return_type, context)?;
                let right = self.analyze_expr_with_context(right, scopes, return_type, context)?;
                let ty = Self::binary_result_type(*op, &left.ty, &right.ty, expr.span)?;
                Ok(ResolvedExpr {
                    ty,
                    is_lvalue: false,
                    is_const: left.is_const && right.is_const,
                })
            }
            ExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                let condition =
                    self.analyze_expr_with_context(condition, scopes, return_type, context)?;
                if condition.ty != SemanticType::Int {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonIntegerExpressionWhereIntegerRequired,
                        expr.span,
                        "conditional expression condition must evaluate to int",
                    ));
                }
                let when_true =
                    self.analyze_expr_with_context(when_true, scopes, return_type, context)?;
                let when_false =
                    self.analyze_expr_with_context(when_false, scopes, return_type, context)?;
                if !types_compatible(&when_true.ty, &when_false.ty) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::ConditionalMustHaveMatchingReturnTypes,
                        expr.span,
                        format!(
                            "conditional expression branches must match: {:?} vs {:?}",
                            when_true.ty, when_false.ty
                        ),
                    ));
                }
                Ok(ResolvedExpr {
                    ty:        when_true.ty,
                    is_lvalue: false,
                    is_const:  when_true.is_const && when_false.is_const,
                })
            }
            ExprKind::Assignment {
                op,
                left,
                right,
            } => {
                let left_resolved =
                    self.analyze_expr_with_context(left, scopes, return_type, context)?;
                if !left_resolved.is_lvalue {
                    return Err(SemanticError::new(
                        CompilerErrorCode::BadLValue,
                        left.span,
                        "left side of assignment must be an lvalue",
                    ));
                }
                let right_resolved =
                    self.analyze_expr_with_context(right, scopes, return_type, context)?;
                let result_type = match op {
                    AssignmentOp::Assign => right_resolved.ty.clone(),
                    AssignmentOp::AssignMinus => Self::binary_result_type(
                        BinaryOp::Subtract,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignPlus => Self::binary_result_type(
                        BinaryOp::Add,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignMultiply => Self::binary_result_type(
                        BinaryOp::Multiply,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignDivide => Self::binary_result_type(
                        BinaryOp::Divide,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignModulus => Self::binary_result_type(
                        BinaryOp::Modulus,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignAnd => Self::binary_result_type(
                        BinaryOp::BooleanAnd,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignXor => Self::binary_result_type(
                        BinaryOp::ExclusiveOr,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignOr => Self::binary_result_type(
                        BinaryOp::InclusiveOr,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignShiftLeft => Self::binary_result_type(
                        BinaryOp::ShiftLeft,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignShiftRight => Self::binary_result_type(
                        BinaryOp::ShiftRight,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                    AssignmentOp::AssignUnsignedShiftRight => Self::binary_result_type(
                        BinaryOp::UnsignedShiftRight,
                        &left_resolved.ty,
                        &right_resolved.ty,
                        expr.span,
                    )?,
                };

                if !types_compatible(&left_resolved.ty, &result_type) {
                    return Err(SemanticError::new(
                        type_mismatch_code(&left_resolved.ty, &result_type),
                        expr.span,
                        format!(
                            "assignment target has type {:?}, expression has type {:?}",
                            left_resolved.ty, result_type
                        ),
                    ));
                }

                Ok(ResolvedExpr {
                    ty:        left_resolved.ty,
                    is_lvalue: false,
                    is_const:  false,
                })
            }
        }
    }
}
