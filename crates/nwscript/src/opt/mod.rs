use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CancellationToken, Cancelled, HirBlock, HirExpr, HirExprKind, HirFunction, HirIfStmt,
    HirModule, HirStmt, HirSwitchStmt, LangSpec, OptimizationFlag, OptimizationFlags, ScriptString,
};

mod constants;
mod instructions;

pub(crate) use constants::{build_constant_env, evaluate_const_expr};
use constants::{build_constant_env_with_cancellation, evaluate_const_expr_with_cancellation};
#[cfg(test)]
pub(crate) use instructions::meld_instructions;
pub(crate) use instructions::melded_instruction;

pub(crate) fn optimize_hir(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    optimizations: OptimizationFlags,
) -> HirModule {
    let mut optimized = hir.clone();

    if enables_dead_branches(optimizations) {
        optimized = trim_dead_branches(&optimized, langspec);
    }

    if enables_dead_functions(optimizations) {
        optimized = eliminate_dead_functions(&optimized);
    }

    optimized
}

pub(crate) fn optimize_hir_with_cancellation(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    optimizations: OptimizationFlags,
    cancellation: &CancellationToken,
) -> Result<HirModule, Cancelled> {
    cancellation.check()?;
    let mut optimized = hir.clone();

    if enables_dead_branches(optimizations) {
        optimized = trim_dead_branches_with_cancellation(&optimized, langspec, cancellation)?;
    }

    if enables_dead_functions(optimizations) {
        optimized = eliminate_dead_functions_with_cancellation(&optimized, cancellation)?;
    }

    cancellation.check()?;
    Ok(optimized)
}

fn enables_dead_functions(optimizations: OptimizationFlags) -> bool {
    optimizations.contains(OptimizationFlag::RemoveDeadCode)
}

fn enables_dead_branches(optimizations: OptimizationFlags) -> bool {
    optimizations.contains(OptimizationFlag::RemoveDeadBranches)
}

fn enables_instruction_melding(optimizations: OptimizationFlags) -> bool {
    optimizations.contains(OptimizationFlag::MeldInstructions)
}

pub(crate) fn optimization_needs_post_codegen_passes(optimizations: OptimizationFlags) -> bool {
    enables_instruction_melding(optimizations)
}

pub(crate) fn optimization_needs_hir_passes(optimizations: OptimizationFlags) -> bool {
    enables_dead_functions(optimizations) || enables_dead_branches(optimizations)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConstValue {
    Int(i32),
    Float(f32),
    String(ScriptString),
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

fn trim_dead_branches_with_cancellation(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    cancellation: &CancellationToken,
) -> Result<HirModule, Cancelled> {
    let constants = build_constant_env_with_cancellation(hir, langspec, cancellation)?;
    let mut optimized = hir.clone();
    for function in &mut optimized.functions {
        cancellation.check()?;
        if let Some(body) = function.body.take() {
            function.body = Some(optimize_block_with_cancellation(
                body,
                &constants,
                cancellation,
            )?);
        }
    }
    Ok(optimized)
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

fn optimize_block_with_cancellation(
    block: HirBlock,
    constants: &BTreeMap<String, ConstValue>,
    cancellation: &CancellationToken,
) -> Result<HirBlock, Cancelled> {
    let mut statements = Vec::with_capacity(block.statements.len());
    for statement in block.statements {
        cancellation.check()?;
        if let Some(statement) =
            optimize_stmt_with_cancellation(statement, constants, cancellation)?
        {
            statements.push(statement);
        }
    }
    Ok(HirBlock {
        span: block.span,
        statements,
    })
}

fn optimize_stmt_with_cancellation(
    statement: HirStmt,
    constants: &BTreeMap<String, ConstValue>,
    cancellation: &CancellationToken,
) -> Result<Option<HirStmt>, Cancelled> {
    cancellation.check()?;
    let optimized = match statement {
        HirStmt::Block(block) => Some(HirStmt::Block(Box::new(optimize_block_with_cancellation(
            *block,
            constants,
            cancellation,
        )?))),
        HirStmt::If(boxed) => {
            let HirIfStmt {
                span,
                condition,
                then_branch,
                else_branch,
            } = *boxed;
            if let Some(ConstValue::Int(value)) =
                evaluate_const_expr_with_cancellation(&condition, constants, cancellation)?
            {
                if value != 0 {
                    return optimize_stmt_with_cancellation(*then_branch, constants, cancellation);
                }
                return match else_branch {
                    Some(branch) => {
                        optimize_stmt_with_cancellation(*branch, constants, cancellation)
                    }
                    None => Ok(None),
                };
            }

            Some(HirStmt::If(Box::new(HirIfStmt {
                span,
                condition,
                then_branch: Box::new(
                    optimize_stmt_with_cancellation(*then_branch, constants, cancellation)?
                        .unwrap_or(HirStmt::Empty(span)),
                ),
                else_branch: match else_branch {
                    Some(branch) => {
                        optimize_stmt_with_cancellation(*branch, constants, cancellation)?
                            .map(Box::new)
                    }
                    None => None,
                },
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
                body: Box::new(
                    optimize_stmt_with_cancellation(*body, constants, cancellation)?
                        .unwrap_or(HirStmt::Empty(span)),
                ),
            })))
        }
        HirStmt::While(mut statement) => {
            statement.body = Box::new(
                optimize_stmt_with_cancellation(*statement.body, constants, cancellation)?
                    .unwrap_or(HirStmt::Empty(statement.span)),
            );
            Some(HirStmt::While(statement))
        }
        HirStmt::DoWhile(mut statement) => {
            statement.body = Box::new(
                optimize_stmt_with_cancellation(*statement.body, constants, cancellation)?
                    .unwrap_or(HirStmt::Empty(statement.span)),
            );
            Some(HirStmt::DoWhile(statement))
        }
        HirStmt::For(mut statement) => {
            statement.body = Box::new(
                optimize_stmt_with_cancellation(*statement.body, constants, cancellation)?
                    .unwrap_or(HirStmt::Empty(statement.span)),
            );
            Some(HirStmt::For(statement))
        }
        other => Some(other),
    };
    Ok(optimized)
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
    let mut function_map = BTreeMap::new();
    for (index, function) in hir.functions.iter().enumerate() {
        function_map
            .entry(function.name.clone())
            .and_modify(|existing: &mut (usize, &HirFunction)| {
                if existing.1.body.is_none() && function.body.is_some() {
                    *existing = (index, function);
                }
            })
            .or_insert((index, function));
    }

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
        enum_defaults: hir.enum_defaults.clone(),
        globals: hir.globals.clone(),
        functions,
    }
}

fn eliminate_dead_functions_with_cancellation(
    hir: &HirModule,
    cancellation: &CancellationToken,
) -> Result<HirModule, Cancelled> {
    let mut function_map = BTreeMap::new();
    for (index, function) in hir.functions.iter().enumerate() {
        cancellation.check()?;
        function_map
            .entry(function.name.clone())
            .and_modify(|existing: &mut (usize, &HirFunction)| {
                if existing.1.body.is_none() && function.body.is_some() {
                    *existing = (index, function);
                }
            })
            .or_insert((index, function));
    }

    let entry_name = function_map.get("main").map(|_| "main").or_else(|| {
        function_map
            .get("StartingConditional")
            .map(|_| "StartingConditional")
    });

    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();

    for global in &hir.globals {
        cancellation.check()?;
        if let Some(initializer) = &global.initializer {
            collect_user_calls_expr_with_cancellation(
                initializer,
                &mut ordered,
                &mut visited,
                &function_map,
                cancellation,
            )?;
        }
    }

    if let Some(entry_name) = entry_name {
        visit_function_with_cancellation(
            entry_name,
            &mut ordered,
            &mut visited,
            &function_map,
            cancellation,
        )?;
    }

    let mut functions = Vec::with_capacity(ordered.len());
    for name in ordered {
        cancellation.check()?;
        if let Some((_, function)) = function_map.get(&name) {
            functions.push((*function).clone());
        }
    }

    Ok(HirModule {
        includes: hir.includes.clone(),
        structs: hir.structs.clone(),
        enum_defaults: hir.enum_defaults.clone(),
        globals: hir.globals.clone(),
        functions,
    })
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
        HirStmt::Expr(expr) | HirStmt::Case(expr) => {
            collect_user_calls_expr(expr, ordered, visited, function_map);
        }
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
        HirExprKind::Match {
            value,
            arms,
        } => {
            collect_user_calls_expr(value, ordered, visited, function_map);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_user_calls_expr(guard, ordered, visited, function_map);
                }
                match &arm.body {
                    crate::HirMatchArmBody::Expr(body) => {
                        collect_user_calls_expr(body, ordered, visited, function_map);
                    }
                    crate::HirMatchArmBody::Block {
                        block,
                        tail,
                        ..
                    } => {
                        for statement in &block.statements {
                            collect_user_calls_stmt(statement, ordered, visited, function_map);
                        }
                        if let Some(tail) = tail {
                            collect_user_calls_expr(tail, ordered, visited, function_map);
                        }
                    }
                }
            }
        }
        HirExprKind::CheckedEnumConversion {
            value,
            fallback,
            ..
        } => {
            collect_user_calls_expr(value, ordered, visited, function_map);
            collect_user_calls_expr(fallback, ordered, visited, function_map);
        }
        HirExprKind::Call {
            target,
            arguments,
        } => {
            // Codegen evaluates call arguments right-to-left; native dead-code
            // discovery follows that emitted order as it assigns function
            // labels.
            for argument in arguments.iter().rev() {
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

fn visit_function_with_cancellation(
    name: &str,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
    cancellation: &CancellationToken,
) -> Result<(), Cancelled> {
    cancellation.check()?;
    if !visited.insert(name.to_string()) {
        return Ok(());
    }

    let Some((_, function)) = function_map.get(name) else {
        return Ok(());
    };
    ordered.push(name.to_string());

    if let Some(body) = &function.body {
        collect_user_calls_block_with_cancellation(
            body,
            ordered,
            visited,
            function_map,
            cancellation,
        )?;
    }
    Ok(())
}

fn collect_user_calls_block_with_cancellation(
    block: &HirBlock,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
    cancellation: &CancellationToken,
) -> Result<(), Cancelled> {
    for statement in &block.statements {
        cancellation.check()?;
        collect_user_calls_stmt_with_cancellation(
            statement,
            ordered,
            visited,
            function_map,
            cancellation,
        )?;
    }
    Ok(())
}

fn collect_user_calls_stmt_with_cancellation(
    statement: &HirStmt,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
    cancellation: &CancellationToken,
) -> Result<(), Cancelled> {
    cancellation.check()?;
    match statement {
        HirStmt::Block(block) => collect_user_calls_block_with_cancellation(
            block,
            ordered,
            visited,
            function_map,
            cancellation,
        )?,
        HirStmt::Declare(statement) => {
            for declarator in &statement.declarators {
                cancellation.check()?;
                if let Some(initializer) = &declarator.initializer {
                    collect_user_calls_expr_with_cancellation(
                        initializer,
                        ordered,
                        visited,
                        function_map,
                        cancellation,
                    )?;
                }
            }
        }
        HirStmt::Expr(expr) | HirStmt::Case(expr) => {
            collect_user_calls_expr_with_cancellation(
                expr,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
        HirStmt::If(statement) => {
            collect_user_calls_expr_with_cancellation(
                &statement.condition,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_stmt_with_cancellation(
                &statement.then_branch,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            if let Some(else_branch) = &statement.else_branch {
                collect_user_calls_stmt_with_cancellation(
                    else_branch,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
        }
        HirStmt::Switch(statement) => {
            collect_user_calls_expr_with_cancellation(
                &statement.condition,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_stmt_with_cancellation(
                &statement.body,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
        HirStmt::Return(statement) => {
            if let Some(value) = &statement.value {
                collect_user_calls_expr_with_cancellation(
                    value,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
        }
        HirStmt::While(statement) => {
            collect_user_calls_expr_with_cancellation(
                &statement.condition,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_stmt_with_cancellation(
                &statement.body,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
        HirStmt::DoWhile(statement) => {
            collect_user_calls_stmt_with_cancellation(
                &statement.body,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_expr_with_cancellation(
                &statement.condition,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
        HirStmt::For(statement) => {
            if let Some(initializer) = &statement.initializer {
                collect_user_calls_expr_with_cancellation(
                    initializer,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
            if let Some(condition) = &statement.condition {
                collect_user_calls_expr_with_cancellation(
                    condition,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
            collect_user_calls_stmt_with_cancellation(
                &statement.body,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            if let Some(update) = &statement.update {
                collect_user_calls_expr_with_cancellation(
                    update,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
        }
        HirStmt::Default(_) | HirStmt::Break(_) | HirStmt::Continue(_) | HirStmt::Empty(_) => {}
    }
    Ok(())
}

fn collect_user_calls_expr_with_cancellation(
    expr: &HirExpr,
    ordered: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
    function_map: &BTreeMap<String, (usize, &HirFunction)>,
    cancellation: &CancellationToken,
) -> Result<(), Cancelled> {
    cancellation.check()?;
    match &expr.kind {
        HirExprKind::Literal(_) | HirExprKind::Value(_) => {}
        HirExprKind::Match {
            value,
            arms,
        } => {
            collect_user_calls_expr_with_cancellation(
                value,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            for arm in arms {
                cancellation.check()?;
                if let Some(guard) = &arm.guard {
                    collect_user_calls_expr_with_cancellation(
                        guard,
                        ordered,
                        visited,
                        function_map,
                        cancellation,
                    )?;
                }
                match &arm.body {
                    crate::HirMatchArmBody::Expr(body) => {
                        collect_user_calls_expr_with_cancellation(
                            body,
                            ordered,
                            visited,
                            function_map,
                            cancellation,
                        )?;
                    }
                    crate::HirMatchArmBody::Block {
                        block,
                        tail,
                        ..
                    } => {
                        collect_user_calls_block_with_cancellation(
                            block,
                            ordered,
                            visited,
                            function_map,
                            cancellation,
                        )?;
                        if let Some(tail) = tail {
                            collect_user_calls_expr_with_cancellation(
                                tail,
                                ordered,
                                visited,
                                function_map,
                                cancellation,
                            )?;
                        }
                    }
                }
            }
        }
        HirExprKind::CheckedEnumConversion {
            value,
            fallback,
            ..
        } => {
            collect_user_calls_expr_with_cancellation(
                value,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_expr_with_cancellation(
                fallback,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
        HirExprKind::Call {
            target,
            arguments,
        } => {
            for argument in arguments.iter().rev() {
                cancellation.check()?;
                collect_user_calls_expr_with_cancellation(
                    argument,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
            if let crate::HirCallTarget::Function(name) = target {
                visit_function_with_cancellation(
                    name,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
        }
        HirExprKind::FieldAccess {
            base, ..
        }
        | HirExprKind::Unary {
            expr: base, ..
        } => collect_user_calls_expr_with_cancellation(
            base,
            ordered,
            visited,
            function_map,
            cancellation,
        )?,
        HirExprKind::Binary {
            left,
            right,
            ..
        } => {
            collect_user_calls_expr_with_cancellation(
                left,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_expr_with_cancellation(
                right,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
        HirExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            collect_user_calls_expr_with_cancellation(
                condition,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_expr_with_cancellation(
                when_true,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
            collect_user_calls_expr_with_cancellation(
                when_false,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
        HirExprKind::Assignment {
            op,
            left,
            right,
        } => {
            if *op != crate::AssignmentOp::Assign {
                collect_user_calls_expr_with_cancellation(
                    left,
                    ordered,
                    visited,
                    function_map,
                    cancellation,
                )?;
            }
            collect_user_calls_expr_with_cancellation(
                right,
                ordered,
                visited,
                function_map,
                cancellation,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
