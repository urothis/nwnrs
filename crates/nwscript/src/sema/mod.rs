use std::collections::{BTreeMap, BTreeSet};

use crate::{
    AssignmentOp, BinaryOp, BlockStmt, BuiltinType, BuiltinValue, CancellationToken,
    CompilerErrorCode, EnumBackingType, Expr, ExprKind, FunctionDecl, LangSpec, Literal,
    MagicLiteral, Script, Stmt, TopLevelItem, TypeKind, TypeSpec, UnaryOp,
};

mod analyzer;
mod helpers;
mod model;

use analyzer::Analyzer;
use helpers::*;
pub use model::*;

/// Performs semantic analysis on one parsed script.
///
/// # Errors
///
/// Returns [`SemanticError`] if the script contains semantic violations.
pub fn analyze_script(
    script: &Script,
    langspec: Option<&LangSpec>,
) -> Result<SemanticModel, SemanticError> {
    analyze_script_with_options(script, langspec, SemanticOptions::default())
}

/// Performs semantic analysis on one parsed script with explicit options.
///
/// # Errors
///
/// Returns [`SemanticError`] if the script contains semantic violations.
pub fn analyze_script_with_options(
    script: &Script,
    langspec: Option<&LangSpec>,
    options: SemanticOptions,
) -> Result<SemanticModel, SemanticError> {
    Analyzer::new(script, langspec, options).analyze()
}

pub(crate) fn analyze_script_with_cancellation(
    script: &Script,
    langspec: Option<&LangSpec>,
    options: SemanticOptions,
    cancellation: &CancellationToken,
) -> Result<SemanticModel, SemanticError> {
    Analyzer::new_with_cancellation(script, langspec, options, cancellation).analyze()
}

#[derive(Debug, Clone, PartialEq)]
enum ConstantValue {
    Int(i32),
    Float(f32),
    String(crate::ScriptString),
    ObjectId(i32),
    ObjectSelf,
    ObjectInvalid,
    LocationInvalid,
    Json(String),
    Vector([f32; 3]),
    Enum {
        name:  String,
        value: SemanticEnumValue,
    },
}

impl ConstantValue {
    fn ty(&self) -> SemanticType {
        match self {
            Self::Int(_) => SemanticType::Int,
            Self::Float(_) => SemanticType::Float,
            Self::String(_) => SemanticType::String,
            Self::ObjectId(_) | Self::ObjectSelf | Self::ObjectInvalid => SemanticType::Object,
            Self::LocationInvalid => SemanticType::EngineStructure("location".to_string()),
            Self::Json(_) => SemanticType::EngineStructure("json".to_string()),
            Self::Vector(_) => SemanticType::Vector,
            Self::Enum {
                name,
                value,
            } => SemanticType::Enum {
                name:    name.clone(),
                backing: match value {
                    SemanticEnumValue::Int(_) => EnumBackingType::Int,
                    SemanticEnumValue::String(_) => EnumBackingType::String,
                },
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ValueBinding {
    Variable {
        ty:       SemanticType,
        is_const: bool,
    },
    Constant(ConstantValue),
}

impl ValueBinding {
    fn ty(&self) -> SemanticType {
        match self {
            Self::Variable {
                ty, ..
            } => ty.clone(),
            Self::Constant(value) => value.ty(),
        }
    }

    fn is_const(&self) -> bool {
        match self {
            Self::Variable {
                is_const, ..
            } => *is_const,
            Self::Constant(_) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopeBinding {
    ty:       SemanticType,
    is_const: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct FunctionInfo {
    signature:        SemanticFunction,
    declaration_span: crate::Span,
}

#[derive(Debug, Default)]
struct AnalysisContext {
    switch_stack: Vec<SwitchContext>,
    loop_depth:   usize,
}

#[derive(Debug)]
struct SwitchContext {
    case_values:       BTreeSet<i32>,
    has_default:       bool,
    scope_decl_counts: Vec<usize>,
    condition_type:    SemanticType,
}

impl AnalysisContext {
    fn enter_scope(&mut self) {
        for switch in &mut self.switch_stack {
            switch.scope_decl_counts.push(0);
        }
    }

    fn exit_scope(&mut self) {
        for switch in &mut self.switch_stack {
            switch.scope_decl_counts.pop();
            if switch.scope_decl_counts.is_empty() {
                switch.scope_decl_counts.push(0);
            }
        }
    }

    fn record_declaration(&mut self) {
        for switch in &mut self.switch_stack {
            if let Some(current) = switch.scope_decl_counts.last_mut() {
                *current += 1;
            }
        }
    }

    fn current_switch_mut(&mut self) -> Option<&mut SwitchContext> {
        self.switch_stack.last_mut()
    }
}

impl SwitchContext {
    fn has_live_declarations(&self) -> bool {
        self.scope_decl_counts.iter().any(|count| *count > 0)
    }
}

#[cfg(test)]
mod tests;
