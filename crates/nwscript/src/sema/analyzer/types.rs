use super::*;

impl Analyzer<'_> {
    pub(super) fn binary_result_type(
        op: BinaryOp,
        left: &SemanticType,
        right: &SemanticType,
        span: crate::Span,
    ) -> Result<SemanticType, SemanticError> {
        let uses_enum =
            matches!(left, SemanticType::Enum { .. }) || matches!(right, SemanticType::Enum { .. });
        let valid_enum_equality = matches!(op, BinaryOp::EqualEqual | BinaryOp::NotEqual)
            && left == right
            && matches!(left, SemanticType::Enum { .. });
        if uses_enum && !valid_enum_equality {
            return Err(SemanticError::new(
                CompilerErrorCode::InvalidEnumOperation,
                span,
                format!(
                    "operation {op:?} is invalid for strong enum operands {left:?} and {right:?}; \
                     convert explicitly to the backing type first"
                ),
            ));
        }
        match op {
            BinaryOp::LogicalAnd
            | BinaryOp::LogicalOr
            | BinaryOp::InclusiveOr
            | BinaryOp::ExclusiveOr
            | BinaryOp::BooleanAnd => {
                if left == &SemanticType::Int && right == &SemanticType::Int {
                    Ok(SemanticType::Int)
                } else {
                    Err(SemanticError::new(
                        CompilerErrorCode::LogicalOperationHasInvalidOperands,
                        span,
                        format!(
                            "logical operation requires int operands, got {left:?} and {right:?}"
                        ),
                    ))
                }
            }
            BinaryOp::EqualEqual | BinaryOp::NotEqual => {
                if left == right
                    && matches!(
                        left,
                        SemanticType::Int
                            | SemanticType::Float
                            | SemanticType::String
                            | SemanticType::Object
                            | SemanticType::Vector
                            | SemanticType::Struct(_)
                            | SemanticType::EngineStructure(_)
                            | SemanticType::Enum { .. }
                    )
                {
                    Ok(SemanticType::Int)
                } else {
                    Err(SemanticError::new(
                        CompilerErrorCode::EqualityTestHasInvalidOperands,
                        span,
                        format!(
                            "equality test requires matching operand types, got {left:?} and \
                             {right:?}"
                        ),
                    ))
                }
            }
            BinaryOp::GreaterEqual
            | BinaryOp::GreaterThan
            | BinaryOp::LessThan
            | BinaryOp::LessEqual => match (left, right) {
                (SemanticType::Int, SemanticType::Int)
                | (SemanticType::Float, SemanticType::Float) => Ok(SemanticType::Int),
                _ => Err(SemanticError::new(
                    CompilerErrorCode::ComparisonTestHasInvalidOperands,
                    span,
                    format!(
                        "comparison requires int/int or float/float operands, got {left:?} and \
                         {right:?}"
                    ),
                )),
            },
            BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::UnsignedShiftRight => {
                if left == &SemanticType::Int && right == &SemanticType::Int {
                    Ok(SemanticType::Int)
                } else {
                    Err(SemanticError::new(
                        CompilerErrorCode::ShiftOperationHasInvalidOperands,
                        span,
                        format!(
                            "shift operation requires int operands, got {left:?} and {right:?}"
                        ),
                    ))
                }
            }
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
                match (left, right) {
                    (SemanticType::Int, SemanticType::Int) => Ok(SemanticType::Int),
                    (SemanticType::Float, SemanticType::Int | SemanticType::Float)
                    | (SemanticType::Int, SemanticType::Float) => Ok(SemanticType::Float),
                    (SemanticType::String, SemanticType::String) if op == BinaryOp::Add => {
                        Ok(SemanticType::String)
                    }
                    (SemanticType::Vector, SemanticType::Vector)
                        if matches!(op, BinaryOp::Add | BinaryOp::Subtract) =>
                    {
                        Ok(SemanticType::Vector)
                    }
                    (SemanticType::Vector, SemanticType::Float)
                        if matches!(op, BinaryOp::Multiply | BinaryOp::Divide) =>
                    {
                        Ok(SemanticType::Vector)
                    }
                    (SemanticType::Float, SemanticType::Vector) if op == BinaryOp::Multiply => {
                        Ok(SemanticType::Vector)
                    }
                    _ => Err(SemanticError::new(
                        CompilerErrorCode::ArithmeticOperationHasInvalidOperands,
                        span,
                        format!(
                            "arithmetic operation {op:?} is invalid for {left:?} and {right:?}"
                        ),
                    )),
                }
            }
            BinaryOp::Modulus => {
                if left == &SemanticType::Int && right == &SemanticType::Int {
                    Ok(SemanticType::Int)
                } else {
                    Err(SemanticError::new(
                        CompilerErrorCode::ArithmeticOperationHasInvalidOperands,
                        span,
                        format!("modulus requires int operands, got {left:?} and {right:?}"),
                    ))
                }
            }
        }
    }

    pub(super) fn resolve_function_signature(
        &self,
        function: &FunctionDecl,
    ) -> Result<SemanticFunction, SemanticError> {
        if function.parameters.len() > MAX_FUNCTION_PARAMETERS {
            return Err(SemanticError::new(
                CompilerErrorCode::TooManyParametersOnFunction,
                function.span,
                format!(
                    "function {:?} has {} parameters; the maximum is {MAX_FUNCTION_PARAMETERS}",
                    function.name,
                    function.parameters.len()
                ),
            ));
        }
        let return_type = self.resolve_type(&function.return_type)?;
        let mut parameters = Vec::new();
        let mut optional_started = false;
        for parameter in &function.parameters {
            let parameter_type = self.resolve_type(&parameter.ty)?;
            let default = if let Some(default) = &parameter.default {
                let value = self
                    .evaluate_function_default_expr(default)
                    .ok_or_else(|| {
                        SemanticError::new(
                            CompilerErrorCode::NonConstantInFunctionDeclaration,
                            default.span,
                            format!(
                                "parameter {:?} default value must be a constant",
                                parameter.name
                            ),
                        )
                    })?;

                if !type_supports_optional_parameter(&parameter_type) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::TypeDoesNotHaveAnOptionalParameter,
                        default.span,
                        format!("type {parameter_type:?} does not support optional parameters"),
                    ));
                }
                if !types_compatible(&parameter_type, &value.ty()) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonConstantInFunctionDeclaration,
                        default.span,
                        format!(
                            "parameter {:?} default type {:?} does not match {:?}",
                            parameter.name,
                            value.ty(),
                            parameter_type
                        ),
                    ));
                }
                optional_started = true;
                Some(value)
            } else {
                if optional_started {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonOptionalParameterCannotFollowOptionalParameter,
                        parameter.span,
                        format!(
                            "parameter {:?} cannot follow an optional parameter",
                            parameter.name
                        ),
                    ));
                }
                None
            };

            parameters.push(SemanticParameter {
                name:        parameter.name.clone(),
                ty:          parameter_type,
                is_optional: default.is_some(),
                default:     default.as_ref().map(literal_from_constant_value),
            });
        }

        Ok(SemanticFunction {
            name: function.name.clone(),
            return_type,
            parameters,
            has_body: function.body.is_some(),
            is_builtin: false,
        })
    }

    pub(super) fn resolve_type(&self, ty: &TypeSpec) -> Result<SemanticType, SemanticError> {
        match &ty.kind {
            TypeKind::Void => Ok(SemanticType::Void),
            TypeKind::Int => Ok(SemanticType::Int),
            TypeKind::Float => Ok(SemanticType::Float),
            TypeKind::String => Ok(SemanticType::String),
            TypeKind::Object => Ok(SemanticType::Object),
            TypeKind::Vector => Ok(SemanticType::Vector),
            TypeKind::Struct(name) => {
                if !self.structs.contains_key(name) && name != "vector" {
                    return Err(SemanticError::new(
                        CompilerErrorCode::UndefinedStructure,
                        ty.span,
                        format!("undefined structure {name:?}"),
                    ));
                }
                if name == "vector" {
                    Ok(SemanticType::Vector)
                } else {
                    Ok(SemanticType::Struct(name.clone()))
                }
            }
            TypeKind::EngineStructure(name) => Ok(SemanticType::EngineStructure(name.clone())),
            TypeKind::Named(name) => self
                .aliases
                .get(name)
                .cloned()
                .or_else(|| {
                    self.enums.get(name).map(|declaration| SemanticType::Enum {
                        name:    name.clone(),
                        backing: declaration.backing,
                    })
                })
                .ok_or_else(|| {
                    SemanticError::new(
                        CompilerErrorCode::InvalidDeclarationType,
                        ty.span,
                        format!("undefined source type {name:?}"),
                    )
                }),
        }
    }

    pub(super) fn evaluate_constant_expr(&self, expr: &Expr) -> Option<ConstantValue> {
        match &expr.kind {
            ExprKind::Literal(literal) => constant_from_literal(literal),
            ExprKind::Identifier(name) => self.lookup_constant(name),
            ExprKind::ScopedIdentifier {
                scope,
                name,
            } => {
                let enum_name = self.canonical_enum_name(scope)?;
                let declaration = self.enums.get(enum_name)?;
                let variant = declaration
                    .variants
                    .iter()
                    .find(|variant| variant.name == *name)?;
                Some(ConstantValue::Enum {
                    name:  enum_name.to_string(),
                    value: variant.value.clone(),
                })
            }
            ExprKind::Unary {
                op: UnaryOp::Negate,
                expr,
            } => {
                let value = self.evaluate_constant_expr(expr)?;
                match value {
                    ConstantValue::Int(value) => Some(ConstantValue::Int(value.wrapping_neg())),
                    ConstantValue::Float(value) => Some(ConstantValue::Float(-value)),
                    _ => None,
                }
            }
            ExprKind::Unary {
                op: UnaryOp::BooleanNot,
                expr,
            } => match self.evaluate_constant_expr(expr)? {
                ConstantValue::Int(value) => Some(ConstantValue::Int(i32::from(value == 0))),
                _ => None,
            },
            ExprKind::Unary {
                op: UnaryOp::OnesComplement,
                expr,
            } => match self.evaluate_constant_expr(expr)? {
                ConstantValue::Int(value) => Some(ConstantValue::Int(!value)),
                _ => None,
            },
            ExprKind::Binary {
                op,
                left,
                right,
            } => self.evaluate_constant_binary(*op, left, right),
            ExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                let condition = self.evaluate_constant_expr(condition)?;
                let take_true = match condition {
                    ConstantValue::Int(value) => value != 0,
                    _ => return None,
                };
                if take_true {
                    self.evaluate_constant_expr(when_true)
                } else {
                    self.evaluate_constant_expr(when_false)
                }
            }
            ExprKind::Match(expression) => {
                let ConstantValue::Enum {
                    name: enum_name,
                    value,
                } = self.evaluate_constant_expr(&expression.value)?
                else {
                    return None;
                };
                let declaration = self.enums.get(&enum_name)?;
                for arm in &expression.arms {
                    let pattern_matches = arm.patterns.iter().any(|pattern| match pattern {
                        crate::MatchPattern::Wildcard {
                            ..
                        } => true,
                        crate::MatchPattern::Variant {
                            scope,
                            name,
                            ..
                        } if scope == &enum_name => declaration
                            .variants
                            .iter()
                            .find(|variant| variant.name == *name)
                            .is_some_and(|variant| variant.value == value),
                        crate::MatchPattern::Variant {
                            ..
                        } => false,
                    });
                    if !pattern_matches {
                        continue;
                    }
                    if let Some(guard) = &arm.guard
                        && !matches!(
                            self.evaluate_constant_expr(guard),
                            Some(ConstantValue::Int(value)) if value != 0
                        )
                    {
                        continue;
                    }
                    return match &arm.body {
                        crate::MatchArmBody::Expr(body) => self.evaluate_constant_expr(body),
                        crate::MatchArmBody::Block(block) if block.statements.is_empty() => block
                            .tail
                            .as_ref()
                            .and_then(|tail| self.evaluate_constant_expr(tail)),
                        crate::MatchArmBody::Block(_) => None,
                    };
                }
                None
            }
            ExprKind::Call {
                callee,
                arguments,
            } if arguments.len() == 1 => {
                let ExprKind::Identifier(name) = &callee.kind else {
                    return None;
                };
                let value = self.evaluate_constant_expr(arguments.first()?)?;
                match (name.as_str(), value) {
                    (
                        "int",
                        ConstantValue::Enum {
                            value: SemanticEnumValue::Int(value),
                            ..
                        },
                    ) => Some(ConstantValue::Int(value)),
                    (
                        "string",
                        ConstantValue::Enum {
                            value: SemanticEnumValue::String(value),
                            ..
                        },
                    ) => Some(ConstantValue::String(value)),
                    (type_name, ConstantValue::Int(value)) => self
                        .enum_constructor_target(type_name, EnumBackingType::Int)
                        .map(|enum_name| ConstantValue::Enum {
                            name:  enum_name,
                            value: SemanticEnumValue::Int(value),
                        }),
                    (type_name, ConstantValue::String(value)) => self
                        .enum_constructor_target(type_name, EnumBackingType::String)
                        .map(|enum_name| ConstantValue::Enum {
                            name:  enum_name,
                            value: SemanticEnumValue::String(value),
                        }),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(super) fn evaluate_function_default_expr(&self, expr: &Expr) -> Option<ConstantValue> {
        self.evaluate_constant_expr(expr)
    }

    pub(super) fn enum_constructor_target(
        &self,
        type_name: &str,
        backing: EnumBackingType,
    ) -> Option<String> {
        if self
            .enums
            .get(type_name)
            .is_some_and(|declaration| declaration.backing == backing)
        {
            return Some(type_name.to_string());
        }
        let SemanticType::Enum {
            name,
            backing: alias_backing,
        } = self.aliases.get(type_name)?
        else {
            return None;
        };
        (*alias_backing == backing).then(|| name.clone())
    }

    pub(super) fn canonical_enum_name<'b>(&'b self, type_name: &'b str) -> Option<&'b str> {
        if self.enums.contains_key(type_name) {
            return Some(type_name);
        }
        let SemanticType::Enum {
            name, ..
        } = self.aliases.get(type_name)?
        else {
            return None;
        };
        Some(name)
    }

    pub(super) fn evaluate_constant_binary(
        &self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
    ) -> Option<ConstantValue> {
        if matches!(op, BinaryOp::LogicalOr | BinaryOp::LogicalAnd)
            && let Some(ConstantValue::Int(left_value)) = self.evaluate_constant_expr(left)
        {
            if op == BinaryOp::LogicalOr && left_value != 0 {
                return Some(ConstantValue::Int(1));
            }
            if op == BinaryOp::LogicalAnd && left_value == 0 {
                return Some(ConstantValue::Int(0));
            }
        }

        let left = self.evaluate_constant_expr(left)?;
        let right = self.evaluate_constant_expr(right)?;

        match (left, right) {
            (ConstantValue::Int(left), ConstantValue::Int(right)) => {
                evaluate_int_constant_binary(op, left, right).map(ConstantValue::Int)
            }
            (ConstantValue::Float(left), ConstantValue::Float(right)) => {
                evaluate_float_constant_binary(op, left, right)
            }
            (ConstantValue::String(left), ConstantValue::String(right)) => {
                evaluate_string_constant_binary(op, &left, &right)
            }
            (
                ConstantValue::Enum {
                    name: left_name,
                    value: left,
                },
                ConstantValue::Enum {
                    name: right_name,
                    value: right,
                },
            ) if left_name == right_name => match op {
                BinaryOp::EqualEqual => Some(ConstantValue::Int(i32::from(left == right))),
                BinaryOp::NotEqual => Some(ConstantValue::Int(i32::from(left != right))),
                _ => None,
            },
            _ => None,
        }
    }

    pub(super) fn evaluate_switch_case_value(&self, expr: &Expr) -> Option<i32> {
        match self.evaluate_constant_expr(expr)? {
            ConstantValue::Int(value) => Some(value),
            ConstantValue::String(value) => {
                Some(crate::nwscript_string_hash_bytes(value.as_bytes()))
            }
            ConstantValue::Enum {
                value: SemanticEnumValue::Int(value),
                ..
            } => Some(value),
            ConstantValue::Enum {
                value: SemanticEnumValue::String(value),
                ..
            } => Some(crate::nwscript_string_hash_bytes(value.as_bytes())),
            _ => None,
        }
    }

    pub(super) fn lookup_constant(&self, name: &str) -> Option<ConstantValue> {
        self.global_constants
            .get(name)
            .cloned()
            .or_else(|| self.builtin_constants.get(name).cloned())
    }

    pub(super) fn lookup_value(
        &self,
        name: &str,
        scopes: &[BTreeMap<String, ScopeBinding>],
    ) -> Option<ValueBinding> {
        for scope in scopes.iter().rev() {
            if let Some(binding) = scope.get(name) {
                return Some(ValueBinding::Variable {
                    ty:       binding.ty.clone(),
                    is_const: binding.is_const,
                });
            }
        }

        if let Some(global) = self.globals.get(name) {
            if global.is_const
                && let Some(value) = self.global_constants.get(name).cloned()
            {
                return Some(ValueBinding::Constant(value));
            }

            return Some(ValueBinding::Variable {
                ty:       global.ty.clone(),
                is_const: global.is_const,
            });
        }

        self.lookup_constant(name).map(ValueBinding::Constant)
    }

    pub(super) fn validate_entrypoint(&self) -> Result<(), SemanticError> {
        if !self.options.require_entrypoint {
            return Ok(());
        }

        if let Some(main) = self.functions.get("main") {
            if main.signature.return_type != SemanticType::Void {
                return Err(SemanticError::new(
                    CompilerErrorCode::FunctionMainMustHaveVoidReturnValue,
                    main.declaration_span,
                    "main must return void",
                ));
            }
            if !main.signature.parameters.is_empty() {
                return Err(SemanticError::new(
                    CompilerErrorCode::FunctionMainMustHaveNoParameters,
                    main.declaration_span,
                    "main must not take parameters",
                ));
            }
            return Ok(());
        }

        if self.options.allow_conditional_script
            && let Some(function) = self.functions.get("StartingConditional")
        {
            if function.signature.return_type != SemanticType::Int {
                return Err(SemanticError::new(
                    CompilerErrorCode::FunctionIntscMustHaveVoidReturnValue,
                    function.declaration_span,
                    "StartingConditional must return int",
                ));
            }
            if !function.signature.parameters.is_empty() {
                return Err(SemanticError::new(
                    CompilerErrorCode::FunctionIntscMustHaveNoParameters,
                    function.declaration_span,
                    "StartingConditional must not take parameters",
                ));
            }
            return Ok(());
        }

        Err(SemanticError::new(
            CompilerErrorCode::NoFunctionMainInScript,
            crate::Span::new(crate::SourceId::new(0), 0, 0),
            "script must define main",
        ))
    }
}
