use std::collections::{BTreeMap, BTreeSet};

use crate::{
    BinaryOp, BuiltinValue, HirBlock, HirExpr, HirExprKind, HirFunction, HirIfStmt, HirModule,
    HirStmt, HirSwitchStmt, LangSpec, Literal, NcsInstruction, NcsOpcode, OptimizationLevel,
    SemanticType, UnaryOp,
};

pub(crate) fn optimize_hir(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    optimization: OptimizationLevel,
) -> HirModule {
    let mut optimized = hir.clone();

    if enables_dead_branches(optimization) {
        optimized = trim_dead_branches(&optimized, langspec);
    }

    if enables_dead_functions(optimization) {
        optimized = eliminate_dead_functions(&optimized);
    }

    optimized
}

pub(crate) fn meld_instructions(instructions: Vec<NcsInstruction>) -> Vec<NcsInstruction> {
    let mut optimized: Vec<NcsInstruction> = Vec::with_capacity(instructions.len());

    for instruction in instructions {
        if instruction.opcode == NcsOpcode::ModifyStackPointer
            && let [.., runstack_add, constant, assignment] = optimized.as_slice()
            && runstack_add.opcode == NcsOpcode::RunstackAdd
            && constant.opcode == NcsOpcode::Constant
            && assignment.opcode == NcsOpcode::Assignment
            && runstack_add.auxcode == constant.auxcode
            && assignment_stack_offset(assignment) == Some(-8)
        {
            let constant = constant.clone();
            optimized.pop();
            optimized.pop();
            optimized.pop();
            optimized.push(constant);
            continue;
        }

        optimized.push(instruction);
    }

    optimized
}

fn enables_dead_functions(optimization: OptimizationLevel) -> bool {
    matches!(
        optimization,
        OptimizationLevel::O1 | OptimizationLevel::O2 | OptimizationLevel::O3
    )
}

fn enables_dead_branches(optimization: OptimizationLevel) -> bool {
    matches!(optimization, OptimizationLevel::O2 | OptimizationLevel::O3)
}

fn enables_instruction_melding(optimization: OptimizationLevel) -> bool {
    optimization == OptimizationLevel::O3
}

pub(crate) fn optimization_needs_post_codegen_passes(optimization: OptimizationLevel) -> bool {
    enables_instruction_melding(optimization)
}

pub(crate) fn optimization_needs_hir_passes(optimization: OptimizationLevel) -> bool {
    enables_dead_functions(optimization) || enables_dead_branches(optimization)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConstValue {
    Int(i32),
    Float(f32),
    String(String),
}

fn trim_dead_branches(hir: &HirModule, langspec: Option<&LangSpec>) -> HirModule {
    let constants = build_constant_env(hir, langspec);
    let mut optimized = hir.clone();
    for function in &mut optimized.functions {
        if let Some(body) = function.body.take() {
            function.body = Some(optimize_block(body, &constants));
        }
    }
    optimized
}

fn optimize_block(block: HirBlock, constants: &BTreeMap<String, ConstValue>) -> HirBlock {
    let mut statements = Vec::with_capacity(block.statements.len());
    for statement in block.statements {
        if let Some(statement) = optimize_stmt(statement, constants) {
            statements.push(statement);
        }
    }
    HirBlock {
        span: block.span,
        statements,
    }
}

fn optimize_stmt(statement: HirStmt, constants: &BTreeMap<String, ConstValue>) -> Option<HirStmt> {
    match statement {
        HirStmt::Block(block) => Some(HirStmt::Block(Box::new(optimize_block(*block, constants)))),
        HirStmt::If(boxed) => {
            let HirIfStmt {
                span,
                condition,
                then_branch,
                else_branch,
            } = *boxed;
            if let Some(ConstValue::Int(value)) = evaluate_const_expr(&condition, constants) {
                if value != 0 {
                    return optimize_stmt(*then_branch, constants);
                }
                return else_branch.and_then(|branch| optimize_stmt(*branch, constants));
            }

            Some(HirStmt::If(Box::new(HirIfStmt {
                span,
                condition,
                then_branch: Box::new(
                    optimize_stmt(*then_branch, constants).unwrap_or(HirStmt::Empty(span)),
                ),
                else_branch: else_branch
                    .and_then(|branch| optimize_stmt(*branch, constants))
                    .map(Box::new),
            })))
        }
        HirStmt::Switch(boxed) => {
            let HirSwitchStmt {
                span,
                condition,
                body,
            } = *boxed;
            Some(HirStmt::Switch(Box::new(HirSwitchStmt {
                span,
                condition,
                body: Box::new(optimize_stmt(*body, constants).unwrap_or(HirStmt::Empty(span))),
            })))
        }
        HirStmt::While(mut statement) => {
            statement.body = Box::new(
                optimize_stmt(*statement.body, constants).unwrap_or(HirStmt::Empty(statement.span)),
            );
            Some(HirStmt::While(statement))
        }
        HirStmt::DoWhile(mut statement) => {
            statement.body = Box::new(
                optimize_stmt(*statement.body, constants).unwrap_or(HirStmt::Empty(statement.span)),
            );
            Some(HirStmt::DoWhile(statement))
        }
        HirStmt::For(mut statement) => {
            statement.body = Box::new(
                optimize_stmt(*statement.body, constants).unwrap_or(HirStmt::Empty(statement.span)),
            );
            Some(HirStmt::For(statement))
        }
        other => Some(other),
    }
}

fn eliminate_dead_functions(hir: &HirModule) -> HirModule {
    let function_map = hir
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| (function.name.clone(), (index, function)))
        .collect::<BTreeMap<_, _>>();

    let entry_name = function_map.get("main").map(|_| "main").or_else(|| {
        function_map
            .get("StartingConditional")
            .map(|_| "StartingConditional")
    });

    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();

    if !hir.globals.is_empty() {
        for global in &hir.globals {
            if let Some(initializer) = &global.initializer {
                collect_user_calls_expr(initializer, &mut ordered, &mut visited, &function_map);
            }
        }
    }

    if let Some(entry_name) = entry_name {
        visit_function(entry_name, &mut ordered, &mut visited, &function_map);
    }

    let mut functions = Vec::with_capacity(ordered.len());
    for name in ordered {
        if let Some((_, function)) = function_map.get(&name) {
            functions.push((*function).clone());
        }
    }

    HirModule {
        includes: hir.includes.clone(),
        structs: hir.structs.clone(),
        globals: hir.globals.clone(),
        functions,
    }
}

fn visit_function(
    name: &str,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
) {
    if !visited.insert(name.to_string()) {
        return;
    }

    let Some((_, function)) = function_map.get(name) else {
        return;
    };
    ordered.push(name.to_string());

    if let Some(body) = &function.body {
        collect_user_calls_block(body, ordered, visited, function_map);
    }
}

fn collect_user_calls_block(
    block: &HirBlock,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
) {
    for statement in &block.statements {
        collect_user_calls_stmt(statement, ordered, visited, function_map);
    }
}

fn collect_user_calls_stmt(
    statement: &HirStmt,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
) {
    match statement {
        HirStmt::Block(block) => collect_user_calls_block(block, ordered, visited, function_map),
        HirStmt::Declare(statement) => {
            for declarator in &statement.declarators {
                if let Some(initializer) = &declarator.initializer {
                    collect_user_calls_expr(initializer, ordered, visited, function_map);
                }
            }
        }
        HirStmt::Expr(expr) => collect_user_calls_expr(expr, ordered, visited, function_map),
        HirStmt::If(statement) => {
            collect_user_calls_expr(&statement.condition, ordered, visited, function_map);
            collect_user_calls_stmt(&statement.then_branch, ordered, visited, function_map);
            if let Some(else_branch) = &statement.else_branch {
                collect_user_calls_stmt(else_branch, ordered, visited, function_map);
            }
        }
        HirStmt::Switch(statement) => {
            collect_user_calls_expr(&statement.condition, ordered, visited, function_map);
            collect_user_calls_stmt(&statement.body, ordered, visited, function_map);
        }
        HirStmt::Return(statement) => {
            if let Some(value) = &statement.value {
                collect_user_calls_expr(value, ordered, visited, function_map);
            }
        }
        HirStmt::While(statement) => {
            collect_user_calls_expr(&statement.condition, ordered, visited, function_map);
            collect_user_calls_stmt(&statement.body, ordered, visited, function_map);
        }
        HirStmt::DoWhile(statement) => {
            collect_user_calls_stmt(&statement.body, ordered, visited, function_map);
            collect_user_calls_expr(&statement.condition, ordered, visited, function_map);
        }
        HirStmt::For(statement) => {
            if let Some(initializer) = &statement.initializer {
                collect_user_calls_expr(initializer, ordered, visited, function_map);
            }
            if let Some(condition) = &statement.condition {
                collect_user_calls_expr(condition, ordered, visited, function_map);
            }
            collect_user_calls_stmt(&statement.body, ordered, visited, function_map);
            if let Some(update) = &statement.update {
                collect_user_calls_expr(update, ordered, visited, function_map);
            }
        }
        HirStmt::Case(expr) => collect_user_calls_expr(expr, ordered, visited, function_map),
        HirStmt::Default(_) | HirStmt::Break(_) | HirStmt::Continue(_) | HirStmt::Empty(_) => {}
    }
}

fn collect_user_calls_expr(
    expr: &HirExpr,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
) {
    match &expr.kind {
        HirExprKind::Literal(_) | HirExprKind::Value(_) => {}
        HirExprKind::Call {
            target,
            arguments,
        } => {
            for argument in arguments {
                collect_user_calls_expr(argument, ordered, visited, function_map);
            }
            if let crate::HirCallTarget::Function(name) = target {
                visit_function(name, ordered, visited, function_map);
            }
        }
        HirExprKind::FieldAccess {
            base, ..
        } => {
            collect_user_calls_expr(base, ordered, visited, function_map);
        }
        HirExprKind::Unary {
            expr, ..
        } => {
            collect_user_calls_expr(expr, ordered, visited, function_map);
        }
        HirExprKind::Binary {
            left,
            right,
            ..
        } => {
            collect_user_calls_expr(left, ordered, visited, function_map);
            collect_user_calls_expr(right, ordered, visited, function_map);
        }
        HirExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            collect_user_calls_expr(condition, ordered, visited, function_map);
            collect_user_calls_expr(when_true, ordered, visited, function_map);
            collect_user_calls_expr(when_false, ordered, visited, function_map);
        }
        HirExprKind::Assignment {
            op,
            left,
            right,
        } => {
            if *op != crate::AssignmentOp::Assign {
                collect_user_calls_expr(left, ordered, visited, function_map);
            }
            collect_user_calls_expr(right, ordered, visited, function_map);
        }
    }
}

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

pub(crate) fn evaluate_const_expr(
    expr: &HirExpr,
    constants: &BTreeMap<String, ConstValue>,
) -> Option<ConstValue> {
    match &expr.kind {
        HirExprKind::Literal(literal) => const_from_literal(literal),
        HirExprKind::Value(
            crate::HirValueRef::ConstGlobal(name) | crate::HirValueRef::BuiltinConstant(name),
        ) => constants.get(name).cloned(),
        HirExprKind::Value(_) => None,
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
        HirExprKind::Call {
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

fn const_from_builtin_value(value: &BuiltinValue) -> Option<ConstValue> {
    match value {
        BuiltinValue::Int(value) => Some(ConstValue::Int(*value)),
        BuiltinValue::Float(value) => Some(ConstValue::Float(*value)),
        BuiltinValue::String(value) => Some(ConstValue::String(value.clone())),
        BuiltinValue::ObjectId(value) => Some(ConstValue::Int(*value)),
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
        SemanticType::String => Some(ConstValue::String(String::new())),
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
        BinaryOp::UnsignedShiftRight => Some(((left.cast_unsigned()).wrapping_shr(right.cast_unsigned())).cast_signed()),
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

fn evaluate_string_binary(op: BinaryOp, left: &str, right: &str) -> Option<ConstValue> {
    match op {
        BinaryOp::Add => Some(ConstValue::String(format!("{left}{right}"))),
        BinaryOp::EqualEqual => Some(ConstValue::Int(i32::from(left == right))),
        BinaryOp::NotEqual => Some(ConstValue::Int(i32::from(left != right))),
        _ => None,
    }
}

fn assignment_stack_offset(instruction: &NcsInstruction) -> Option<i32> {
    let bytes = instruction.extra.get(0..4)?;
    let mut offset = [0u8; 4];
    offset.copy_from_slice(bytes);
    Some(i32::from_be_bytes(offset))
}

#[cfg(test)]
mod tests {
    use super::{OptimizationLevel, meld_instructions, optimize_hir};
    use crate::{
        BuiltinConstant, BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue, LangSpec,
        NcsAuxCode, NcsInstruction, NcsOpcode, SourceId, compile_hir_to_ncs,
        decode_ncs_instructions, lower_to_hir, parse_text,
    };

    fn test_langspec() -> LangSpec {
        LangSpec {
            engine_num_structures: 0,
            engine_structures:     vec![],
            constants:             vec![
                BuiltinConstant {
                    name:  "TRUE".to_string(),
                    ty:    BuiltinType::Int,
                    value: BuiltinValue::Int(1),
                },
                BuiltinConstant {
                    name:  "FALSE".to_string(),
                    ty:    BuiltinType::Int,
                    value: BuiltinValue::Int(0),
                },
            ],
            functions:             vec![BuiltinFunction {
                name:        "GetValue".to_string(),
                return_type: BuiltinType::Int,
                parameters:  vec![BuiltinParameter {
                    name:    "nValue".to_string(),
                    ty:      BuiltinType::Int,
                    default: None,
                }],
            }],
        }
    }

    #[test]
    fn o1_eliminates_dead_functions_in_loader_discovery_order() {
        let script = parse_text(
            SourceId::new(1),
            r#"
                int NeverUsed() { return 9; }
                int Leaf() { return 7; }
                int Mid() { return Leaf(); }
                int StartingConditional() { return Mid(); }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let semantic =
            crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
        let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
            .expect("HIR lowering should succeed");

        let optimized = optimize_hir(&hir, Some(&test_langspec()), OptimizationLevel::O1);
        let names = optimized
            .functions
            .iter()
            .map(|function| function.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["StartingConditional", "Mid", "Leaf"]);
    }

    #[test]
    fn o2_dead_branch_trimming_removes_calls_from_constant_false_if_branches() {
        let script = parse_text(
            SourceId::new(2),
            r#"
                int Dead() { return 0; }
                int Live() { return 1; }
                int StartingConditional() {
                    if (FALSE) {
                        return Dead();
                    } else {
                        return Live();
                    }
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let semantic =
            crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
        let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
            .expect("HIR lowering should succeed");

        let optimized = optimize_hir(&hir, Some(&test_langspec()), OptimizationLevel::O2);
        let names = optimized
            .functions
            .iter()
            .map(|function| function.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["StartingConditional", "Live"]);
    }

    #[test]
    fn o3_melds_local_constant_initializer_pattern() {
        let script = parse_text(
            SourceId::new(3),
            r#"
                void main() {
                    int nValue = 3;
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let semantic =
            crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
        let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
            .expect("HIR lowering should succeed");

        let o0 = decode_ncs_instructions(
            &compile_hir_to_ncs(&hir, Some(&test_langspec()), OptimizationLevel::O0)
                .expect("O0 compile should succeed"),
        )
        .expect("O0 output should decode");
        let o3 = decode_ncs_instructions(
            &compile_hir_to_ncs(&hir, Some(&test_langspec()), OptimizationLevel::O3)
                .expect("O3 compile should succeed"),
        )
        .expect("O3 output should decode");

        assert!(
            o0.iter()
                .any(|instruction| instruction.opcode == NcsOpcode::RunstackAdd),
            "O0 should preserve the local initializer stack dance",
        );
        assert!(
            !o3.iter().any(|instruction| {
                instruction.opcode == NcsOpcode::Assignment
                    && instruction.auxcode == NcsAuxCode::TypeVoid
            }),
            "O3 should meld the local constant initializer pattern",
        );
    }

    #[test]
    fn meld_instructions_only_rewrites_the_upstream_active_pattern() {
        let instructions = vec![
            NcsInstruction {
                opcode:  NcsOpcode::RunstackAdd,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   3_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Assignment,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   {
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(&(-8_i32).to_be_bytes());
                    bytes.extend_from_slice(&(4_u16).to_be_bytes());
                    bytes
                },
            },
            NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-4_i32).to_be_bytes().to_vec(),
            },
        ];

        let optimized = meld_instructions(instructions);
        assert_eq!(optimized.len(), 1);
        assert_eq!(
            optimized.first().map(|instruction| instruction.opcode),
            Some(NcsOpcode::Constant)
        );
    }
}
