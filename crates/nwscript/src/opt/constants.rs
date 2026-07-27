use std::collections::BTreeMap;

use super::ConstValue;
use crate::{
    BinaryOp, BuiltinValue, CancellationToken, Cancelled, HirExpr, HirExprKind, HirModule,
    LangSpec, Literal, ScriptString, SemanticType, UnaryOp,
};

pub(crate) fn build_constant_env(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
) -> BTreeMap<String, ConstValue> {
    let mut constants = BTreeMap::new();
    if let Some(langspec) = langspec {
        for constant in &langspec.constants {
            if let Some(value) = const_from_builtin_value(&constant.value) {
                constants.insert(constant.name.clone(), value);
            }
        }
    }

    for global in &hir.globals {
        if !global.is_const {
            continue;
        }
        let value = global
            .initializer
            .as_ref()
            .and_then(|initializer| evaluate_const_expr(initializer, &constants))
            .or_else(|| default_const_value(&global.ty));
        if let Some(value) = value {
            constants.insert(global.name.clone(), value);
        }
    }

    constants
}

pub(super) fn build_constant_env_with_cancellation(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    cancellation: &CancellationToken,
) -> Result<BTreeMap<String, ConstValue>, Cancelled> {
    let mut constants = BTreeMap::new();
    if let Some(langspec) = langspec {
        for constant in &langspec.constants {
            cancellation.check()?;
            if let Some(value) = const_from_builtin_value(&constant.value) {
                constants.insert(constant.name.clone(), value);
            }
        }
    }

    for global in &hir.globals {
        cancellation.check()?;
        if !global.is_const {
            continue;
        }
        let value = match &global.initializer {
            Some(initializer) => {
                evaluate_const_expr_with_cancellation(initializer, &constants, cancellation)?
                    .or_else(|| default_const_value(&global.ty))
            }
            None => default_const_value(&global.ty),
        };
        if let Some(value) = value {
            constants.insert(global.name.clone(), value);
        }
    }

    Ok(constants)
}

pub(crate) fn evaluate_const_expr(
    expr: &HirExpr,
    constants: &BTreeMap<String, ConstValue>,
) -> Option<ConstValue> {
    match &expr.kind {
        HirExprKind::Literal(literal) => const_from_literal(literal),
        HirExprKind::Value(
            crate::HirValueRef::ConstGlobal(name) | crate::HirValueRef::BuiltinConstant(name),
        ) => constants.get(name).cloned(),
        HirExprKind::Unary {
            op,
            expr,
        } => {
            let value = evaluate_const_expr(expr, constants)?;
            match (op, value) {
                (UnaryOp::Negate, ConstValue::Int(value)) => Some(ConstValue::Int(-value)),
                (UnaryOp::Negate, ConstValue::Float(value)) => Some(ConstValue::Float(-value)),
                (UnaryOp::BooleanNot, ConstValue::Int(value)) => {
                    Some(ConstValue::Int(i32::from(value == 0)))
                }
                (UnaryOp::OnesComplement, ConstValue::Int(value)) => Some(ConstValue::Int(!value)),
                _ => None,
            }
        }
        HirExprKind::Binary {
            op,
            left,
            right,
        } => {
            let left = evaluate_const_expr(left, constants)?;
            let right = evaluate_const_expr(right, constants)?;
            match (left, right) {
                (ConstValue::Int(left), ConstValue::Int(right)) => {
                    evaluate_int_binary(*op, left, right).map(ConstValue::Int)
                }
                (ConstValue::Float(left), ConstValue::Float(right)) => {
                    evaluate_float_binary(*op, left, right)
                }
                (ConstValue::String(left), ConstValue::String(right)) => {
                    evaluate_string_binary(*op, &left, &right)
                }
                _ => None,
            }
        }
        HirExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => match evaluate_const_expr(condition, constants)? {
            ConstValue::Int(value) if value != 0 => evaluate_const_expr(when_true, constants),
            ConstValue::Int(_) => evaluate_const_expr(when_false, constants),
            _ => None,
        },
        HirExprKind::Match {
            value,
            arms,
        } => {
            let value = evaluate_const_expr(value, constants)?;
            for arm in arms {
                let pattern_matches = arm.patterns.is_empty()
                    || arm.patterns.iter().any(|pattern| {
                        const_from_literal(pattern).is_some_and(|pattern| pattern == value)
                    });
                if !pattern_matches {
                    continue;
                }
                if let Some(guard) = &arm.guard
                    && !matches!(evaluate_const_expr(guard, constants), Some(ConstValue::Int(value)) if value != 0)
                {
                    continue;
                }
                return match &arm.body {
                    crate::HirMatchArmBody::Expr(body) => evaluate_const_expr(body, constants),
                    crate::HirMatchArmBody::Block {
                        block,
                        tail,
                        ..
                    } if block.statements.is_empty() => tail
                        .as_ref()
                        .and_then(|tail| evaluate_const_expr(tail, constants)),
                    crate::HirMatchArmBody::Block {
                        ..
                    } => None,
                };
            }
            None
        }
        HirExprKind::CheckedEnumConversion {
            value,
            fallback,
            valid_values,
        } => {
            let value = evaluate_const_expr(value, constants)?;
            if valid_values
                .iter()
                .filter_map(const_from_literal)
                .any(|valid| valid == value)
            {
                Some(value)
            } else {
                evaluate_const_expr(fallback, constants)
            }
        }
        HirExprKind::Value(_)
        | HirExprKind::Call {
            ..
        }
        | HirExprKind::FieldAccess {
            ..
        }
        | HirExprKind::Assignment {
            ..
        } => None,
    }
}

pub(super) fn evaluate_const_expr_with_cancellation(
    expr: &HirExpr,
    constants: &BTreeMap<String, ConstValue>,
    cancellation: &CancellationToken,
) -> Result<Option<ConstValue>, Cancelled> {
    cancellation.check()?;
    let value = match &expr.kind {
        HirExprKind::Literal(literal) => const_from_literal(literal),
        HirExprKind::Value(
            crate::HirValueRef::ConstGlobal(name) | crate::HirValueRef::BuiltinConstant(name),
        ) => constants.get(name).cloned(),
        HirExprKind::Unary {
            op,
            expr,
        } => {
            let Some(value) = evaluate_const_expr_with_cancellation(expr, constants, cancellation)?
            else {
                return Ok(None);
            };
            match (op, value) {
                (UnaryOp::Negate, ConstValue::Int(value)) => Some(ConstValue::Int(-value)),
                (UnaryOp::Negate, ConstValue::Float(value)) => Some(ConstValue::Float(-value)),
                (UnaryOp::BooleanNot, ConstValue::Int(value)) => {
                    Some(ConstValue::Int(i32::from(value == 0)))
                }
                (UnaryOp::OnesComplement, ConstValue::Int(value)) => Some(ConstValue::Int(!value)),
                _ => None,
            }
        }
        HirExprKind::Binary {
            op,
            left,
            right,
        } => {
            let Some(left) = evaluate_const_expr_with_cancellation(left, constants, cancellation)?
            else {
                return Ok(None);
            };
            let Some(right) =
                evaluate_const_expr_with_cancellation(right, constants, cancellation)?
            else {
                return Ok(None);
            };
            match (left, right) {
                (ConstValue::Int(left), ConstValue::Int(right)) => {
                    evaluate_int_binary(*op, left, right).map(ConstValue::Int)
                }
                (ConstValue::Float(left), ConstValue::Float(right)) => {
                    evaluate_float_binary(*op, left, right)
                }
                (ConstValue::String(left), ConstValue::String(right)) => {
                    evaluate_string_binary(*op, &left, &right)
                }
                _ => None,
            }
        }
        HirExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            let Some(condition) =
                evaluate_const_expr_with_cancellation(condition, constants, cancellation)?
            else {
                return Ok(None);
            };
            match condition {
                ConstValue::Int(value) if value != 0 => {
                    evaluate_const_expr_with_cancellation(when_true, constants, cancellation)?
                }
                ConstValue::Int(_) => {
                    evaluate_const_expr_with_cancellation(when_false, constants, cancellation)?
                }
                _ => None,
            }
        }
        HirExprKind::Match {
            value,
            arms,
        } => {
            let Some(value) =
                evaluate_const_expr_with_cancellation(value, constants, cancellation)?
            else {
                return Ok(None);
            };
            let mut selected = None;
            for arm in arms {
                cancellation.check()?;
                let pattern_matches = arm.patterns.is_empty()
                    || arm.patterns.iter().any(|pattern| {
                        const_from_literal(pattern).is_some_and(|pattern| pattern == value)
                    });
                if !pattern_matches {
                    continue;
                }
                if let Some(guard) = &arm.guard {
                    let guard =
                        evaluate_const_expr_with_cancellation(guard, constants, cancellation)?;
                    if !matches!(guard, Some(ConstValue::Int(value)) if value != 0) {
                        continue;
                    }
                }
                selected = match &arm.body {
                    crate::HirMatchArmBody::Expr(body) => {
                        evaluate_const_expr_with_cancellation(body, constants, cancellation)?
                    }
                    crate::HirMatchArmBody::Block {
                        block,
                        tail,
                        ..
                    } if block.statements.is_empty() => match tail {
                        Some(tail) => {
                            evaluate_const_expr_with_cancellation(tail, constants, cancellation)?
                        }
                        None => None,
                    },
                    crate::HirMatchArmBody::Block {
                        ..
                    } => None,
                };
                break;
            }
            selected
        }
        HirExprKind::CheckedEnumConversion {
            value,
            fallback,
            valid_values,
        } => {
            let Some(value) =
                evaluate_const_expr_with_cancellation(value, constants, cancellation)?
            else {
                return Ok(None);
            };
            if valid_values
                .iter()
                .filter_map(const_from_literal)
                .any(|valid| valid == value)
            {
                Some(value)
            } else {
                evaluate_const_expr_with_cancellation(fallback, constants, cancellation)?
            }
        }
        HirExprKind::Value(_)
        | HirExprKind::Call {
            ..
        }
        | HirExprKind::FieldAccess {
            ..
        }
        | HirExprKind::Assignment {
            ..
        } => None,
    };
    Ok(value)
}

fn const_from_builtin_value(value: &BuiltinValue) -> Option<ConstValue> {
    match value {
        BuiltinValue::Int(value) | BuiltinValue::ObjectId(value) => Some(ConstValue::Int(*value)),
        BuiltinValue::Float(value) => Some(ConstValue::Float(*value)),
        BuiltinValue::String(value) => Some(ConstValue::String(value.clone())),
        BuiltinValue::ObjectSelf => Some(ConstValue::Int(0)),
        BuiltinValue::ObjectInvalid => Some(ConstValue::Int(1)),
        BuiltinValue::LocationInvalid
        | BuiltinValue::Json(_)
        | BuiltinValue::Vector(_)
        | BuiltinValue::Raw(_) => None,
    }
}

fn const_from_literal(literal: &Literal) -> Option<ConstValue> {
    match literal {
        Literal::Integer(value) => Some(ConstValue::Int(*value)),
        Literal::Float(value) => Some(ConstValue::Float(*value)),
        Literal::String(value) => Some(ConstValue::String(value.clone())),
        Literal::ObjectSelf => Some(ConstValue::Int(0)),
        Literal::ObjectInvalid => Some(ConstValue::Int(1)),
        Literal::LocationInvalid | Literal::Json(_) | Literal::Vector(_) | Literal::Magic(_) => {
            None
        }
    }
}

fn default_const_value(ty: &SemanticType) -> Option<ConstValue> {
    match ty {
        SemanticType::Int => Some(ConstValue::Int(0)),
        SemanticType::Float => Some(ConstValue::Float(0.0)),
        SemanticType::String => Some(ConstValue::String(ScriptString::default())),
        _ => None,
    }
}

fn evaluate_int_binary(op: BinaryOp, left: i32, right: i32) -> Option<i32> {
    match op {
        BinaryOp::Multiply => Some(left.wrapping_mul(right)),
        BinaryOp::Divide => (right != 0).then_some(left.wrapping_div(right)),
        BinaryOp::Modulus => (right != 0).then_some(left.wrapping_rem(right)),
        BinaryOp::Add => Some(left.wrapping_add(right)),
        BinaryOp::Subtract => Some(left.wrapping_sub(right)),
        BinaryOp::ShiftLeft => Some(left.wrapping_shl(right.cast_unsigned())),
        BinaryOp::ShiftRight => Some(left.wrapping_shr(right.cast_unsigned())),
        BinaryOp::UnsignedShiftRight => {
            Some(((left.cast_unsigned()).wrapping_shr(right.cast_unsigned())).cast_signed())
        }
        BinaryOp::GreaterEqual => Some(i32::from(left >= right)),
        BinaryOp::GreaterThan => Some(i32::from(left > right)),
        BinaryOp::LessThan => Some(i32::from(left < right)),
        BinaryOp::LessEqual => Some(i32::from(left <= right)),
        BinaryOp::NotEqual => Some(i32::from(left != right)),
        BinaryOp::EqualEqual => Some(i32::from(left == right)),
        BinaryOp::BooleanAnd => Some(left & right),
        BinaryOp::ExclusiveOr => Some(left ^ right),
        BinaryOp::InclusiveOr => Some(left | right),
        BinaryOp::LogicalAnd => Some(i32::from((left != 0) && (right != 0))),
        BinaryOp::LogicalOr => Some(i32::from((left != 0) || (right != 0))),
    }
}

#[allow(clippy::float_cmp)]
fn evaluate_float_binary(op: BinaryOp, left: f32, right: f32) -> Option<ConstValue> {
    match op {
        BinaryOp::Multiply => Some(ConstValue::Float(left * right)),
        BinaryOp::Divide => Some(ConstValue::Float(left / right)),
        BinaryOp::Add => Some(ConstValue::Float(left + right)),
        BinaryOp::Subtract => Some(ConstValue::Float(left - right)),
        BinaryOp::GreaterEqual => Some(ConstValue::Int(i32::from(left >= right))),
        BinaryOp::GreaterThan => Some(ConstValue::Int(i32::from(left > right))),
        BinaryOp::LessThan => Some(ConstValue::Int(i32::from(left < right))),
        BinaryOp::LessEqual => Some(ConstValue::Int(i32::from(left <= right))),
        BinaryOp::NotEqual => Some(ConstValue::Int(i32::from(left != right))),
        BinaryOp::EqualEqual => Some(ConstValue::Int(i32::from(left == right))),
        _ => None,
    }
}

fn evaluate_string_binary(
    op: BinaryOp,
    left: &ScriptString,
    right: &ScriptString,
) -> Option<ConstValue> {
    match op {
        BinaryOp::Add => Some(ConstValue::String(left.concat(right))),
        BinaryOp::EqualEqual => Some(ConstValue::Int(i32::from(left == right))),
        BinaryOp::NotEqual => Some(ConstValue::Int(i32::from(left != right))),
        _ => None,
    }
}
