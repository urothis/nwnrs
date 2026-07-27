use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedExpr {
    pub(super) ty:        SemanticType,
    pub(super) is_lvalue: bool,
    pub(super) is_const:  bool,
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

pub(super) fn constant_from_builtin_value(value: &BuiltinValue) -> Option<ConstantValue> {
    match value {
        BuiltinValue::Int(value) => Some(ConstantValue::Int(*value)),
        BuiltinValue::Float(value) => Some(ConstantValue::Float(*value)),
        BuiltinValue::String(value) => Some(ConstantValue::String(value.clone())),
        BuiltinValue::ObjectId(value) => Some(ConstantValue::ObjectId(*value)),
        BuiltinValue::ObjectSelf => Some(ConstantValue::ObjectSelf),
        BuiltinValue::ObjectInvalid => Some(ConstantValue::ObjectInvalid),
        BuiltinValue::LocationInvalid => Some(ConstantValue::LocationInvalid),
        BuiltinValue::Json(value) => Some(ConstantValue::Json(value.clone())),
        BuiltinValue::Vector(value) => Some(ConstantValue::Vector(*value)),
        BuiltinValue::Raw(_) => None,
    }
}

pub(super) fn literal_from_builtin_value(value: &BuiltinValue) -> Option<Literal> {
    constant_from_builtin_value(value).map(|value| literal_from_constant_value(&value))
}

pub(super) fn constant_from_literal(literal: &Literal) -> Option<ConstantValue> {
    match literal {
        Literal::Integer(value) => Some(ConstantValue::Int(*value)),
        Literal::Float(value) => Some(ConstantValue::Float(*value)),
        Literal::String(value) => Some(ConstantValue::String(value.clone())),
        Literal::ObjectSelf => Some(ConstantValue::ObjectSelf),
        Literal::ObjectInvalid => Some(ConstantValue::ObjectInvalid),
        Literal::LocationInvalid => Some(ConstantValue::LocationInvalid),
        Literal::Json(value) => Some(ConstantValue::Json(value.clone())),
        Literal::Vector(value) => Some(ConstantValue::Vector(*value)),
        Literal::Magic(
            MagicLiteral::Function
            | MagicLiteral::File
            | MagicLiteral::Line
            | MagicLiteral::Date
            | MagicLiteral::Time,
        ) => None,
    }
}

pub(super) fn literal_from_constant_value(value: &ConstantValue) -> Literal {
    match value {
        ConstantValue::Int(value) | ConstantValue::ObjectId(value) => Literal::Integer(*value),
        ConstantValue::Float(value) => Literal::Float(*value),
        ConstantValue::String(value) => Literal::String(value.clone()),
        ConstantValue::ObjectSelf => Literal::ObjectSelf,
        ConstantValue::ObjectInvalid => Literal::ObjectInvalid,
        ConstantValue::LocationInvalid => Literal::LocationInvalid,
        ConstantValue::Json(value) => Literal::Json(value.clone()),
        ConstantValue::Vector(value) => Literal::Vector(*value),
        ConstantValue::Enum {
            value: SemanticEnumValue::Int(value),
            ..
        } => Literal::Integer(*value),
        ConstantValue::Enum {
            value: SemanticEnumValue::String(value),
            ..
        } => Literal::String(value.clone()),
    }
}

pub(super) fn semantic_type_from_literal(literal: &Literal) -> SemanticType {
    match literal {
        Literal::Integer(_) | Literal::Magic(MagicLiteral::Line) => SemanticType::Int,
        Literal::Float(_) => SemanticType::Float,
        Literal::String(_)
        | Literal::Magic(
            MagicLiteral::Function | MagicLiteral::File | MagicLiteral::Date | MagicLiteral::Time,
        ) => SemanticType::String,
        Literal::ObjectSelf | Literal::ObjectInvalid => SemanticType::Object,
        Literal::LocationInvalid => SemanticType::EngineStructure("location".to_string()),
        Literal::Json(_) => SemanticType::EngineStructure("json".to_string()),
        Literal::Vector(_) => SemanticType::Vector,
    }
}

pub(super) fn default_constant_value(
    ty: &SemanticType,
    enums: &BTreeMap<String, SemanticEnum>,
) -> Option<ConstantValue> {
    match ty {
        SemanticType::Int => Some(ConstantValue::Int(0)),
        SemanticType::Float => Some(ConstantValue::Float(0.0)),
        SemanticType::String => Some(ConstantValue::String(crate::ScriptString::default())),
        SemanticType::Enum {
            name, ..
        } => {
            let declaration = enums.get(name)?;
            let variant = declaration.variants.get(declaration.default_variant)?;
            Some(ConstantValue::Enum {
                name:  name.clone(),
                value: variant.value.clone(),
            })
        }
        _ => None,
    }
}

pub(super) fn evaluate_int_constant_binary(op: BinaryOp, left: i32, right: i32) -> Option<i32> {
    match op {
        BinaryOp::LogicalOr => Some(i32::from(left != 0 || right != 0)),
        BinaryOp::LogicalAnd => Some(i32::from(left != 0 && right != 0)),
        BinaryOp::InclusiveOr => Some(left | right),
        BinaryOp::ExclusiveOr => Some(left ^ right),
        BinaryOp::BooleanAnd => Some(left & right),
        BinaryOp::EqualEqual => Some(i32::from(left == right)),
        BinaryOp::NotEqual => Some(i32::from(left != right)),
        BinaryOp::GreaterEqual => Some(i32::from(left >= right)),
        BinaryOp::GreaterThan => Some(i32::from(left > right)),
        BinaryOp::LessThan => Some(i32::from(left < right)),
        BinaryOp::LessEqual => Some(i32::from(left <= right)),
        BinaryOp::ShiftLeft => Some(left.wrapping_shl(right.cast_unsigned())),
        BinaryOp::ShiftRight => Some(left.wrapping_shr(right.cast_unsigned())),
        BinaryOp::UnsignedShiftRight => {
            Some(((left.cast_unsigned()).wrapping_shr(right.cast_unsigned())).cast_signed())
        }
        BinaryOp::Add => Some(left.wrapping_add(right)),
        BinaryOp::Subtract => Some(left.wrapping_sub(right)),
        BinaryOp::Multiply => Some(left.wrapping_mul(right)),
        BinaryOp::Divide => left.checked_div(right),
        BinaryOp::Modulus => left.checked_rem(right),
    }
}

#[allow(clippy::float_cmp)]
pub(super) fn evaluate_float_constant_binary(
    op: BinaryOp,
    left: f32,
    right: f32,
) -> Option<ConstantValue> {
    match op {
        BinaryOp::Add => Some(ConstantValue::Float(left + right)),
        BinaryOp::Subtract => Some(ConstantValue::Float(left - right)),
        BinaryOp::Multiply => Some(ConstantValue::Float(left * right)),
        BinaryOp::Divide => Some(ConstantValue::Float(left / right)),
        BinaryOp::EqualEqual => Some(ConstantValue::Int(i32::from(left == right))),
        BinaryOp::NotEqual => Some(ConstantValue::Int(i32::from(left != right))),
        BinaryOp::GreaterEqual => Some(ConstantValue::Int(i32::from(left >= right))),
        BinaryOp::GreaterThan => Some(ConstantValue::Int(i32::from(left > right))),
        BinaryOp::LessThan => Some(ConstantValue::Int(i32::from(left < right))),
        BinaryOp::LessEqual => Some(ConstantValue::Int(i32::from(left <= right))),
        _ => None,
    }
}

pub(super) fn evaluate_string_constant_binary(
    op: BinaryOp,
    left: &crate::ScriptString,
    right: &crate::ScriptString,
) -> Option<ConstantValue> {
    match op {
        BinaryOp::Add => {
            if left.len().saturating_add(right.len()) >= 0x8000 {
                None
            } else {
                Some(ConstantValue::String(left.concat(right)))
            }
        }
        BinaryOp::EqualEqual => Some(ConstantValue::Int(i32::from(left == right))),
        BinaryOp::NotEqual => Some(ConstantValue::Int(i32::from(left != right))),
        _ => None,
    }
}

pub(super) fn type_supports_optional_parameter(ty: &SemanticType) -> bool {
    match ty {
        SemanticType::Int
        | SemanticType::Float
        | SemanticType::String
        | SemanticType::Object
        | SemanticType::Vector
        | SemanticType::Enum {
            ..
        } => true,
        SemanticType::EngineStructure(name) => name == "location" || name == "json",
        _ => false,
    }
}

pub(super) fn types_compatible(expected: &SemanticType, actual: &SemanticType) -> bool {
    expected == actual
}

pub(super) fn type_mismatch_code(
    expected: &SemanticType,
    actual: &SemanticType,
) -> CompilerErrorCode {
    if matches!(expected, SemanticType::Enum { .. }) || matches!(actual, SemanticType::Enum { .. })
    {
        CompilerErrorCode::InvalidEnumOperation
    } else {
        CompilerErrorCode::MismatchedTypes
    }
}

pub(super) fn parameters_match(left: &[SemanticParameter], right: &[SemanticParameter]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.ty == right.ty)
}

pub(super) fn insert_scope_binding(
    scopes: &mut [BTreeMap<String, ScopeBinding>],
    name: &str,
    ty: SemanticType,
    is_const: bool,
    span: crate::Span,
) -> Result<(), SemanticError> {
    if current_scope_contains(scopes, name) {
        return Err(SemanticError::new(
            CompilerErrorCode::VariableAlreadyUsedWithinScope,
            span,
            format!("variable {name:?} was already declared in this scope"),
        ));
    }

    let Some(scope) = scopes.last_mut() else {
        return Err(SemanticError::new(
            CompilerErrorCode::UnknownStateInCompiler,
            span,
            "scope stack must be non-empty",
        ));
    };
    scope.insert(
        name.to_string(),
        ScopeBinding {
            ty,
            is_const,
        },
    );
    Ok(())
}

pub(super) fn current_scope_contains(
    scopes: &[BTreeMap<String, ScopeBinding>],
    name: &str,
) -> bool {
    scopes.last().is_some_and(|scope| scope.contains_key(name))
}

pub(super) fn statement_guarantees_return(statement: &Stmt) -> bool {
    match statement {
        Stmt::Return(_) => true,
        Stmt::Expression(statement) => match &statement.expr.kind {
            ExprKind::Match(expression) => expression.arms.iter().all(|arm| match &arm.body {
                crate::MatchArmBody::Block(block) => {
                    block.statements.iter().any(statement_guarantees_return)
                }
                crate::MatchArmBody::Expr(_) => false,
            }),
            _ => false,
        },
        Stmt::Block(block) => {
            for statement in &block.statements {
                if statement_guarantees_return(statement) {
                    return true;
                }
            }
            false
        }
        Stmt::If(statement) => match &statement.else_branch {
            Some(else_branch) => {
                statement_guarantees_return(&statement.then_branch)
                    && statement_guarantees_return(else_branch)
            }
            None => false,
        },
        _ => false,
    }
}

pub(super) fn match_block_guarantees_exit(block: &crate::MatchBlock) -> bool {
    block.statements.iter().any(statement_guarantees_exit)
}

pub(super) fn statement_guarantees_exit(statement: &Stmt) -> bool {
    match statement {
        Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Block(block) => block.statements.iter().any(statement_guarantees_exit),
        Stmt::If(statement) => statement.else_branch.as_ref().is_some_and(|else_branch| {
            statement_guarantees_exit(&statement.then_branch)
                && statement_guarantees_exit(else_branch)
        }),
        _ => false,
    }
}
