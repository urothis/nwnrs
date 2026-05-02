use std::{collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{
    AssignmentOp, BinaryOp, BlockStmt, BuiltinValue, Expr, ExprKind, FunctionDecl,
    IncludeDirective, LangSpec, Literal, Script, SemanticModel, SemanticType, Stmt, StructDecl,
    TypeSpec, UnaryOp,
};

/// One lowered HIR module ready for further compiler passes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirModule {
    /// Top-level includes preserved from the source unit.
    pub includes:  Vec<IncludeDirective>,
    /// User-defined structures in source order.
    pub structs:   Vec<HirStruct>,
    /// Globals in source order.
    pub globals:   Vec<HirGlobal>,
    /// Functions in source order.
    pub functions: Vec<HirFunction>,
}

/// One lowered structure definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirStruct {
    /// Source span.
    pub span:   crate::Span,
    /// Structure name.
    pub name:   String,
    /// Fields in declaration order.
    pub fields: Vec<HirField>,
}

/// One lowered structure field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirField {
    /// Field name.
    pub name: String,
    /// Field type.
    pub ty:   SemanticType,
}

/// One lowered global variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirGlobal {
    /// Source span.
    pub span:        crate::Span,
    /// Global name.
    pub name:        String,
    /// Global type.
    pub ty:          SemanticType,
    /// Whether the global is `const`.
    pub is_const:    bool,
    /// Optional initializer.
    pub initializer: Option<HirExpr>,
}

/// One lowered function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirFunction {
    /// Source span.
    pub span:        crate::Span,
    /// Function name.
    pub name:        String,
    /// Return type.
    pub return_type: SemanticType,
    /// Parameters in declaration order.
    pub parameters:  Vec<HirParameter>,
    /// All local slots, including parameters first.
    pub locals:      Vec<HirLocal>,
    /// Optional body for declarations vs implementations.
    pub body:        Option<HirBlock>,
    /// Whether the function came from the builtin langspec.
    pub is_builtin:  bool,
}

/// One lowered function parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirParameter {
    /// Local slot for this parameter.
    pub local:       HirLocalId,
    /// Parameter name.
    pub name:        String,
    /// Parameter type.
    pub ty:          SemanticType,
    /// Whether the parameter has a default value.
    pub is_optional: bool,
    /// Lowered default value for omitted trailing arguments.
    pub default:     Option<HirExpr>,
}

/// One lowered local slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirLocal {
    /// Stable local id within one function.
    pub id:   HirLocalId,
    /// Local name.
    pub name: String,
    /// Local type.
    pub ty:   SemanticType,
    /// Whether this slot is a parameter or a body-local.
    pub kind: HirLocalKind,
}

/// One lowered local kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirLocalKind {
    /// One parameter slot.
    Parameter,
    /// One block-local slot.
    Local,
}

/// One function-local identifier.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct HirLocalId(pub u32);

/// One lowered block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirBlock {
    /// Source span.
    pub span:       crate::Span,
    /// Lowered statements.
    pub statements: Vec<HirStmt>,
}

/// One lowered statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirStmt {
    /// One nested block.
    Block(Box<HirBlock>),
    /// One local declaration.
    Declare(Box<HirDeclareStmt>),
    /// One expression statement.
    Expr(Box<HirExpr>),
    /// One `if` statement.
    If(Box<HirIfStmt>),
    /// One `switch` statement.
    Switch(Box<HirSwitchStmt>),
    /// One `return` statement.
    Return(Box<HirReturnStmt>),
    /// One `while` statement.
    While(Box<HirWhileStmt>),
    /// One `do/while` statement.
    DoWhile(Box<HirDoWhileStmt>),
    /// One `for` statement.
    For(Box<HirForStmt>),
    /// One `case` label.
    Case(Box<HirExpr>),
    /// One `default` label.
    Default(crate::Span),
    /// One `break`.
    Break(crate::Span),
    /// One `continue`.
    Continue(crate::Span),
    /// One empty statement.
    Empty(crate::Span),
}

/// One lowered declaration statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirDeclareStmt {
    /// Source span.
    pub span:        crate::Span,
    /// Declaration type.
    pub ty:          SemanticType,
    /// Declared locals in source order.
    pub declarators: Vec<HirDeclarator>,
}

/// One lowered declared local.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirDeclarator {
    /// Local slot for the declared value.
    pub local:       HirLocalId,
    /// Optional initializer.
    pub initializer: Option<HirExpr>,
}

/// One lowered `if`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirIfStmt {
    /// Source span.
    pub span:        crate::Span,
    /// Condition.
    pub condition:   HirExpr,
    /// True branch.
    pub then_branch: Box<HirStmt>,
    /// Optional false branch.
    pub else_branch: Option<Box<HirStmt>>,
}

/// One lowered `switch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirSwitchStmt {
    /// Source span.
    pub span:      crate::Span,
    /// Switch condition.
    pub condition: HirExpr,
    /// Switch body.
    pub body:      Box<HirStmt>,
}

/// One lowered `return`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirReturnStmt {
    /// Source span.
    pub span:  crate::Span,
    /// Optional return value.
    pub value: Option<HirExpr>,
}

/// One lowered `while`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirWhileStmt {
    /// Source span.
    pub span:      crate::Span,
    /// Loop condition.
    pub condition: HirExpr,
    /// Loop body.
    pub body:      Box<HirStmt>,
}

/// One lowered `do/while`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirDoWhileStmt {
    /// Source span.
    pub span:      crate::Span,
    /// Loop body.
    pub body:      Box<HirStmt>,
    /// Loop condition.
    pub condition: HirExpr,
}

/// One lowered `for`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirForStmt {
    /// Source span.
    pub span:        crate::Span,
    /// Optional initializer expression.
    pub initializer: Option<HirExpr>,
    /// Optional condition expression.
    pub condition:   Option<HirExpr>,
    /// Optional update expression.
    pub update:      Option<HirExpr>,
    /// Loop body.
    pub body:        Box<HirStmt>,
}

/// One lowered expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirExpr {
    /// Source span.
    pub span: crate::Span,
    /// Resolved expression type.
    pub ty:   SemanticType,
    /// Lowered expression kind.
    pub kind: HirExprKind,
}

/// One lowered expression kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirExprKind {
    /// One literal constant.
    Literal(Literal),
    /// One resolved value reference.
    Value(HirValueRef),
    /// One direct function call.
    Call {
        /// Resolved target.
        target:    HirCallTarget,
        /// Lowered arguments.
        arguments: Vec<HirExpr>,
    },
    /// One structure field access.
    FieldAccess {
        /// Resolved base value.
        base:  Box<HirExpr>,
        /// Field name.
        field: String,
    },
    /// One unary expression.
    Unary {
        /// Operator.
        op:   UnaryOp,
        /// Operand.
        expr: Box<HirExpr>,
    },
    /// One binary expression.
    Binary {
        /// Operator.
        op:    BinaryOp,
        /// Left operand.
        left:  Box<HirExpr>,
        /// Right operand.
        right: Box<HirExpr>,
    },
    /// One conditional expression.
    Conditional {
        /// Condition.
        condition:  Box<HirExpr>,
        /// True branch.
        when_true:  Box<HirExpr>,
        /// False branch.
        when_false: Box<HirExpr>,
    },
    /// One assignment expression.
    Assignment {
        /// Operator.
        op:    AssignmentOp,
        /// Left lvalue.
        left:  Box<HirExpr>,
        /// Right expression.
        right: Box<HirExpr>,
    },
}

/// One resolved value reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirValueRef {
    /// One local or parameter slot.
    Local(HirLocalId),
    /// One mutable or non-const global.
    Global(String),
    /// One const global.
    ConstGlobal(String),
    /// One builtin constant from the langspec.
    BuiltinConstant(String),
}

/// One resolved call target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirCallTarget {
    /// One user-defined function.
    Function(String),
    /// One builtin function.
    Builtin(String),
}

/// One HIR lowering failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirLowerError {
    /// Source span that triggered the failure.
    pub span:    crate::Span,
    /// Human-readable message.
    pub message: String,
}

impl HirLowerError {
    fn new(span: crate::Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for HirLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for HirLowerError {}

/// Lowers one semantically-valid script into typed HIR.
///
/// # Errors
///
/// Returns [`HirLowerError`] if the script cannot be lowered.
pub fn lower_to_hir(
    script: &Script,
    semantic: &SemanticModel,
    langspec: Option<&LangSpec>,
) -> Result<HirModule, HirLowerError> {
    HirLowerer::new(script, semantic, langspec).lower()
}

struct HirLowerer<'a> {
    script:            &'a Script,
    semantic:          &'a SemanticModel,
    builtin_constants: BTreeMap<String, SemanticType>,
}

impl<'a> HirLowerer<'a> {
    fn new(script: &'a Script, semantic: &'a SemanticModel, langspec: Option<&LangSpec>) -> Self {
        let mut builtin_constants = BTreeMap::new();
        if let Some(langspec) = langspec {
            for constant in &langspec.constants {
                if let Some(ty) = semantic_type_from_builtin_value(&constant.value) {
                    builtin_constants.insert(constant.name.clone(), ty);
                }
            }
        }

        Self {
            script,
            semantic,
            builtin_constants,
        }
    }

    fn lower(self) -> Result<HirModule, HirLowerError> {
        let mut includes = Vec::new();
        let mut structs = Vec::new();
        let mut globals = Vec::new();
        let mut functions = Vec::new();

        for item in &self.script.items {
            match item {
                crate::TopLevelItem::Include(include) => includes.push(include.clone()),
                crate::TopLevelItem::Struct(definition) => {
                    structs.push(self.lower_struct(definition)?);
                }
                crate::TopLevelItem::Global(declaration) => {
                    globals.extend(self.lower_global_declaration(declaration)?);
                }
                crate::TopLevelItem::Function(function) => {
                    functions.push(self.lower_function(function)?);
                }
            }
        }

        Ok(HirModule {
            includes,
            structs,
            globals,
            functions,
        })
    }

    fn lower_struct(&self, definition: &StructDecl) -> Result<HirStruct, HirLowerError> {
        let resolved = self
            .semantic
            .structs
            .get(&definition.name)
            .ok_or_else(|| HirLowerError::new(definition.span, "missing semantic struct"))?;
        Ok(HirStruct {
            span:   definition.span,
            name:   resolved.name.clone(),
            fields: resolved
                .fields
                .iter()
                .map(|field| HirField {
                    name: field.name.clone(),
                    ty:   field.ty.clone(),
                })
                .collect(),
        })
    }

    fn lower_global_declaration(
        &self,
        declaration: &crate::Declaration,
    ) -> Result<Vec<HirGlobal>, HirLowerError> {
        let mut globals = Vec::new();
        for declarator in &declaration.declarators {
            let resolved =
                self.semantic.globals.get(&declarator.name).ok_or_else(|| {
                    HirLowerError::new(declarator.span, "missing semantic global")
                })?;
            let initializer = declarator
                .initializer
                .as_ref()
                .map(|initializer| {
                    let mut ctx = FunctionLoweringContext::default();
                    self.lower_expr(initializer, &mut ctx)
                })
                .transpose()?;
            globals.push(HirGlobal {
                span: declarator.span,
                name: resolved.name.clone(),
                ty: resolved.ty.clone(),
                is_const: resolved.is_const,
                initializer,
            });
        }
        Ok(globals)
    }

    fn lower_function(&self, function: &FunctionDecl) -> Result<HirFunction, HirLowerError> {
        let resolved = self
            .semantic
            .functions
            .get(&function.name)
            .ok_or_else(|| HirLowerError::new(function.span, "missing semantic function"))?;
        let mut ctx = FunctionLoweringContext::default();
        ctx.push_scope();
        let mut parameters = Vec::new();
        debug_assert_eq!(resolved.parameters.len(), function.parameters.len());

        for (parameter, parsed) in resolved.parameters.iter().zip(&function.parameters) {
            let local = ctx.push_local(&parsed.name, parameter.ty.clone(), HirLocalKind::Parameter);
            let default = if let Some(default) = &parsed.default {
                Some(self.lower_expr(default, &mut ctx)?)
            } else {
                parameter.default.as_ref().map(|literal| HirExpr {
                    span: function.span,
                    ty:   parameter.ty.clone(),
                    kind: HirExprKind::Literal(literal.clone()),
                })
            };
            parameters.push(HirParameter {
                local,
                name: parsed.name.clone(),
                ty: parameter.ty.clone(),
                is_optional: parameter.is_optional,
                default,
            });
        }
        ctx.push_scope();

        let body = function
            .body
            .as_ref()
            .map(|body| self.lower_block(body, &mut ctx, true))
            .transpose()?;

        Ok(HirFunction {
            span: function.span,
            name: resolved.name.clone(),
            return_type: resolved.return_type.clone(),
            parameters,
            locals: ctx.locals,
            body,
            is_builtin: resolved.is_builtin,
        })
    }

    fn lower_block(
        &self,
        block: &BlockStmt,
        ctx: &mut FunctionLoweringContext,
        is_function_body: bool,
    ) -> Result<HirBlock, HirLowerError> {
        if !is_function_body {
            ctx.push_scope();
        }

        let mut statements = Vec::new();
        for statement in &block.statements {
            statements.push(self.lower_stmt(statement, ctx)?);
        }

        if !is_function_body {
            ctx.pop_scope();
        }

        Ok(HirBlock {
            span: block.span,
            statements,
        })
    }

    fn lower_stmt(
        &self,
        statement: &Stmt,
        ctx: &mut FunctionLoweringContext,
    ) -> Result<HirStmt, HirLowerError> {
        match statement {
            Stmt::Block(block) => Ok(HirStmt::Block(Box::new(
                self.lower_block(block, ctx, false)?,
            ))),
            Stmt::Declaration(declaration) => {
                let ty = lower_decl_type(&declaration.ty, self.semantic)?;
                let mut declarators = Vec::new();
                for declarator in &declaration.declarators {
                    let initializer = declarator
                        .initializer
                        .as_ref()
                        .map(|initializer| self.lower_expr(initializer, ctx))
                        .transpose()?;
                    let local = ctx.push_local(&declarator.name, ty.clone(), HirLocalKind::Local);
                    declarators.push(HirDeclarator {
                        local,
                        initializer,
                    });
                }
                Ok(HirStmt::Declare(Box::new(HirDeclareStmt {
                    span: declaration.span,
                    ty,
                    declarators,
                })))
            }
            Stmt::Expression(statement) => Ok(HirStmt::Expr(Box::new(
                self.lower_expr(&statement.expr, ctx)?,
            ))),
            Stmt::If(statement) => Ok(HirStmt::If(Box::new(HirIfStmt {
                span:        statement.span,
                condition:   self.lower_expr(&statement.condition, ctx)?,
                then_branch: Box::new(self.lower_stmt(&statement.then_branch, ctx)?),
                else_branch: statement
                    .else_branch
                    .as_ref()
                    .map(|branch| self.lower_stmt(branch, ctx).map(Box::new))
                    .transpose()?,
            }))),
            Stmt::Switch(statement) => Ok(HirStmt::Switch(Box::new(HirSwitchStmt {
                span:      statement.span,
                condition: self.lower_expr(&statement.condition, ctx)?,
                body:      Box::new(self.lower_stmt(&statement.body, ctx)?),
            }))),
            Stmt::Return(statement) => Ok(HirStmt::Return(Box::new(HirReturnStmt {
                span:  statement.span,
                value: statement
                    .value
                    .as_ref()
                    .map(|value| self.lower_expr(value, ctx))
                    .transpose()?,
            }))),
            Stmt::While(statement) => Ok(HirStmt::While(Box::new(HirWhileStmt {
                span:      statement.span,
                condition: self.lower_expr(&statement.condition, ctx)?,
                body:      Box::new(self.lower_stmt(&statement.body, ctx)?),
            }))),
            Stmt::DoWhile(statement) => Ok(HirStmt::DoWhile(Box::new(HirDoWhileStmt {
                span:      statement.span,
                body:      Box::new(self.lower_stmt(&statement.body, ctx)?),
                condition: self.lower_expr(&statement.condition, ctx)?,
            }))),
            Stmt::For(statement) => Ok(HirStmt::For(Box::new(HirForStmt {
                span:        statement.span,
                initializer: statement
                    .initializer
                    .as_ref()
                    .map(|expr| self.lower_expr(expr, ctx))
                    .transpose()?,
                condition:   statement
                    .condition
                    .as_ref()
                    .map(|expr| self.lower_expr(expr, ctx))
                    .transpose()?,
                update:      statement
                    .update
                    .as_ref()
                    .map(|expr| self.lower_expr(expr, ctx))
                    .transpose()?,
                body:        Box::new(self.lower_stmt(&statement.body, ctx)?),
            }))),
            Stmt::Case(statement) => Ok(HirStmt::Case(Box::new(
                self.lower_expr(&statement.value, ctx)?,
            ))),
            Stmt::Default(statement) => Ok(HirStmt::Default(statement.span)),
            Stmt::Break(statement) => Ok(HirStmt::Break(statement.span)),
            Stmt::Continue(statement) => Ok(HirStmt::Continue(statement.span)),
            Stmt::Empty(statement) => Ok(HirStmt::Empty(statement.span)),
        }
    }

    fn lower_expr(
        &self,
        expr: &Expr,
        ctx: &mut FunctionLoweringContext,
    ) -> Result<HirExpr, HirLowerError> {
        let lowered = match &expr.kind {
            ExprKind::Literal(literal) => HirExpr {
                span: expr.span,
                ty:   semantic_type_from_literal(literal),
                kind: HirExprKind::Literal(literal.clone()),
            },
            ExprKind::Identifier(name) => {
                if let Some(local) = ctx.lookup_local(name) {
                    HirExpr {
                        span: expr.span,
                        ty:   local.ty.clone(),
                        kind: HirExprKind::Value(HirValueRef::Local(local.id)),
                    }
                } else if let Some(global) = self.semantic.globals.get(name) {
                    HirExpr {
                        span: expr.span,
                        ty:   global.ty.clone(),
                        kind: HirExprKind::Value(if global.is_const {
                            HirValueRef::ConstGlobal(name.clone())
                        } else {
                            HirValueRef::Global(name.clone())
                        }),
                    }
                } else if let Some(ty) = self.builtin_constants.get(name) {
                    HirExpr {
                        span: expr.span,
                        ty:   ty.clone(),
                        kind: HirExprKind::Value(HirValueRef::BuiltinConstant(name.clone())),
                    }
                } else {
                    return Err(HirLowerError::new(
                        expr.span,
                        format!("unresolved value reference {name:?}"),
                    ));
                }
            }
            ExprKind::Call {
                callee,
                arguments,
            } => {
                let ExprKind::Identifier(name) = &callee.kind else {
                    return Err(HirLowerError::new(
                        callee.span,
                        "HIR lowering only supports direct identifier calls",
                    ));
                };
                let function = self.semantic.functions.get(name).ok_or_else(|| {
                    HirLowerError::new(callee.span, "missing semantic call target")
                })?;
                HirExpr {
                    span: expr.span,
                    ty:   function.return_type.clone(),
                    kind: HirExprKind::Call {
                        target:    if function.is_builtin {
                            HirCallTarget::Builtin(name.clone())
                        } else {
                            HirCallTarget::Function(name.clone())
                        },
                        arguments: arguments
                            .iter()
                            .map(|argument| self.lower_expr(argument, ctx))
                            .collect::<Result<Vec<_>, _>>()?,
                    },
                }
            }
            ExprKind::FieldAccess {
                base,
                field,
            } => {
                let base = self.lower_expr(base, ctx)?;
                let ty = field_result_type(&base.ty, field, self.semantic, expr.span)?;
                HirExpr {
                    span: expr.span,
                    ty,
                    kind: HirExprKind::FieldAccess {
                        base:  Box::new(base),
                        field: field.clone(),
                    },
                }
            }
            ExprKind::Unary {
                op,
                expr: inner,
            } => {
                let inner = self.lower_expr(inner, ctx)?;
                let ty = unary_result_type(*op, &inner.ty, expr.span)?;
                HirExpr {
                    span: expr.span,
                    ty,
                    kind: HirExprKind::Unary {
                        op:   *op,
                        expr: Box::new(inner),
                    },
                }
            }
            ExprKind::Binary {
                op,
                left,
                right,
            } => {
                let left = self.lower_expr(left, ctx)?;
                let right = self.lower_expr(right, ctx)?;
                let ty = binary_result_type(*op, &left.ty, &right.ty, expr.span)?;
                HirExpr {
                    span: expr.span,
                    ty,
                    kind: HirExprKind::Binary {
                        op:    *op,
                        left:  Box::new(left),
                        right: Box::new(right),
                    },
                }
            }
            ExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                let condition = self.lower_expr(condition, ctx)?;
                let when_true = self.lower_expr(when_true, ctx)?;
                let when_false = self.lower_expr(when_false, ctx)?;
                HirExpr {
                    span: expr.span,
                    ty:   when_true.ty.clone(),
                    kind: HirExprKind::Conditional {
                        condition:  Box::new(condition),
                        when_true:  Box::new(when_true),
                        when_false: Box::new(when_false),
                    },
                }
            }
            ExprKind::Assignment {
                op,
                left,
                right,
            } => {
                let left = self.lower_expr(left, ctx)?;
                let right = self.lower_expr(right, ctx)?;
                HirExpr {
                    span: expr.span,
                    ty:   assignment_result_type(*op, &left.ty, &right.ty, expr.span)?,
                    kind: HirExprKind::Assignment {
                        op:    *op,
                        left:  Box::new(left),
                        right: Box::new(right),
                    },
                }
            }
        };

        Ok(lowered)
    }
}

#[derive(Default)]
struct FunctionLoweringContext {
    locals: Vec<HirLocal>,
    scopes: Vec<BTreeMap<String, HirLocalBinding>>,
}

#[derive(Clone)]
struct HirLocalBinding {
    id: HirLocalId,
    ty: SemanticType,
}

impl FunctionLoweringContext {
    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn push_local(&mut self, name: &str, ty: SemanticType, kind: HirLocalKind) -> HirLocalId {
        if self.scopes.is_empty() {
            self.push_scope();
        }

        let id = HirLocalId(u32::try_from(self.locals.len()).ok().unwrap_or(u32::MAX));
        self.locals.push(HirLocal {
            id,
            name: name.to_string(),
            ty: ty.clone(),
            kind,
        });
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.to_string(),
                HirLocalBinding {
                    id,
                    ty,
                },
            );
        }
        id
    }

    fn lookup_local(&self, name: &str) -> Option<&HirLocalBinding> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }
}

fn lower_decl_type(ty: &TypeSpec, semantic: &SemanticModel) -> Result<SemanticType, HirLowerError> {
    match &ty.kind {
        crate::TypeKind::Void => Ok(SemanticType::Void),
        crate::TypeKind::Int => Ok(SemanticType::Int),
        crate::TypeKind::Float => Ok(SemanticType::Float),
        crate::TypeKind::String => Ok(SemanticType::String),
        crate::TypeKind::Object => Ok(SemanticType::Object),
        crate::TypeKind::Vector => Ok(SemanticType::Vector),
        crate::TypeKind::Struct(name) => semantic
            .structs
            .contains_key(name)
            .then(|| SemanticType::Struct(name.clone()))
            .ok_or_else(|| HirLowerError::new(ty.span, "missing semantic struct type")),
        crate::TypeKind::EngineStructure(name) => Ok(SemanticType::EngineStructure(name.clone())),
    }
}

fn semantic_type_from_literal(literal: &Literal) -> SemanticType {
    match literal {
        Literal::Integer(_) | Literal::Magic(crate::MagicLiteral::Line) => SemanticType::Int,
        Literal::Float(_) => SemanticType::Float,
        Literal::String(_)
        | Literal::Magic(
            crate::MagicLiteral::Function
            | crate::MagicLiteral::File
            | crate::MagicLiteral::Date
            | crate::MagicLiteral::Time,
        ) => SemanticType::String,
        Literal::ObjectSelf | Literal::ObjectInvalid => SemanticType::Object,
        Literal::LocationInvalid => SemanticType::EngineStructure("location".to_string()),
        Literal::Json(_) => SemanticType::EngineStructure("json".to_string()),
        Literal::Vector(_) => SemanticType::Vector,
    }
}

fn semantic_type_from_builtin_value(value: &BuiltinValue) -> Option<SemanticType> {
    match value {
        BuiltinValue::Int(_) => Some(SemanticType::Int),
        BuiltinValue::Float(_) => Some(SemanticType::Float),
        BuiltinValue::String(_) => Some(SemanticType::String),
        BuiltinValue::ObjectId(_) | BuiltinValue::ObjectSelf | BuiltinValue::ObjectInvalid => {
            Some(SemanticType::Object)
        }
        BuiltinValue::LocationInvalid => {
            Some(SemanticType::EngineStructure("location".to_string()))
        }
        BuiltinValue::Json(_) => Some(SemanticType::EngineStructure("json".to_string())),
        BuiltinValue::Vector(_) => Some(SemanticType::Vector),
        BuiltinValue::Raw(_) => None,
    }
}

fn unary_result_type(
    op: UnaryOp,
    operand: &SemanticType,
    span: crate::Span,
) -> Result<SemanticType, HirLowerError> {
    match op {
        UnaryOp::Negate => match operand {
            SemanticType::Int | SemanticType::Float => Ok(operand.clone()),
            _ => Err(HirLowerError::new(
                span,
                "negation requires an int or float operand",
            )),
        },
        UnaryOp::OnesComplement
        | UnaryOp::BooleanNot
        | UnaryOp::PreIncrement
        | UnaryOp::PreDecrement
        | UnaryOp::PostIncrement
        | UnaryOp::PostDecrement => Ok(SemanticType::Int),
    }
}

fn binary_result_type(
    op: BinaryOp,
    left: &SemanticType,
    right: &SemanticType,
    span: crate::Span,
) -> Result<SemanticType, HirLowerError> {
    match op {
        BinaryOp::LogicalAnd
        | BinaryOp::LogicalOr
        | BinaryOp::InclusiveOr
        | BinaryOp::ExclusiveOr
        | BinaryOp::BooleanAnd
        | BinaryOp::ShiftLeft
        | BinaryOp::ShiftRight
        | BinaryOp::UnsignedShiftRight
        | BinaryOp::Modulus
            if left == &SemanticType::Int && right == &SemanticType::Int =>
        {
            Ok(SemanticType::Int)
        }
        BinaryOp::EqualEqual | BinaryOp::NotEqual
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
                ) =>
        {
            Ok(SemanticType::Int)
        }
        BinaryOp::GreaterEqual
        | BinaryOp::GreaterThan
        | BinaryOp::LessThan
        | BinaryOp::LessEqual
            if matches!(
                (left, right),
                (SemanticType::Int, SemanticType::Int) | (SemanticType::Float, SemanticType::Float)
            ) =>
        {
            Ok(SemanticType::Int)
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
                _ => Err(HirLowerError::new(
                    span,
                    format!("cannot lower binary operation {op:?} for {left:?} and {right:?}"),
                )),
            }
        }
        _ => Err(HirLowerError::new(
            span,
            format!("cannot lower binary operation {op:?} for {left:?} and {right:?}"),
        )),
    }
}

fn assignment_result_type(
    op: AssignmentOp,
    left: &SemanticType,
    right: &SemanticType,
    span: crate::Span,
) -> Result<SemanticType, HirLowerError> {
    match op {
        AssignmentOp::Assign => Ok(left.clone()),
        AssignmentOp::AssignMinus => binary_result_type(BinaryOp::Subtract, left, right, span),
        AssignmentOp::AssignPlus => binary_result_type(BinaryOp::Add, left, right, span),
        AssignmentOp::AssignMultiply => binary_result_type(BinaryOp::Multiply, left, right, span),
        AssignmentOp::AssignDivide => binary_result_type(BinaryOp::Divide, left, right, span),
        AssignmentOp::AssignModulus => binary_result_type(BinaryOp::Modulus, left, right, span),
        AssignmentOp::AssignAnd => binary_result_type(BinaryOp::BooleanAnd, left, right, span),
        AssignmentOp::AssignXor => binary_result_type(BinaryOp::ExclusiveOr, left, right, span),
        AssignmentOp::AssignOr => binary_result_type(BinaryOp::InclusiveOr, left, right, span),
        AssignmentOp::AssignShiftLeft => binary_result_type(BinaryOp::ShiftLeft, left, right, span),
        AssignmentOp::AssignShiftRight => {
            binary_result_type(BinaryOp::ShiftRight, left, right, span)
        }
        AssignmentOp::AssignUnsignedShiftRight => {
            binary_result_type(BinaryOp::UnsignedShiftRight, left, right, span)
        }
    }
}

fn field_result_type(
    base: &SemanticType,
    field: &str,
    semantic: &SemanticModel,
    span: crate::Span,
) -> Result<SemanticType, HirLowerError> {
    match base {
        SemanticType::Vector => match field {
            "x" | "y" | "z" => Ok(SemanticType::Float),
            _ => Err(HirLowerError::new(
                span,
                format!("field {field:?} does not exist on vector"),
            )),
        },
        SemanticType::Struct(name) => semantic
            .structs
            .get(name)
            .and_then(|structure| {
                structure
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == field)
                    .map(|candidate| candidate.ty.clone())
            })
            .ok_or_else(|| {
                HirLowerError::new(
                    span,
                    format!("field {field:?} does not exist on structure {name:?}"),
                )
            }),
        _ => Err(HirLowerError::new(
            span,
            "left side of field access must be a structure",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{HirCallTarget, HirExprKind, HirLocalKind, HirStmt, HirValueRef, lower_to_hir};
    use crate::{
        BuiltinConstant, BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue, LangSpec,
        SourceId, analyze_script, parse_text,
    };

    fn test_langspec() -> LangSpec {
        LangSpec {
            engine_num_structures: 3,
            engine_structures:     vec![
                "effect".to_string(),
                "location".to_string(),
                "json".to_string(),
            ],
            constants:             vec![
                BuiltinConstant {
                    name:  "TRUE".to_string(),
                    ty:    BuiltinType::Int,
                    value: BuiltinValue::Int(1),
                },
                BuiltinConstant {
                    name:  "OBJECT_INVALID".to_string(),
                    ty:    BuiltinType::Object,
                    value: BuiltinValue::ObjectInvalid,
                },
            ],
            functions:             vec![BuiltinFunction {
                name:        "DelayCommand".to_string(),
                return_type: BuiltinType::Void,
                parameters:  vec![
                    BuiltinParameter {
                        name:    "fSeconds".to_string(),
                        ty:      BuiltinType::Float,
                        default: None,
                    },
                    BuiltinParameter {
                        name:    "aAction".to_string(),
                        ty:      BuiltinType::Action,
                        default: None,
                    },
                ],
            }],
        }
    }

    #[test]
    fn lowers_globals_and_locals_to_resolved_value_refs() -> Result<(), Box<dyn std::error::Error>>
    {
        let script = parse_text(
            SourceId::new(70),
            r#"
                int g = TRUE;
                void main() {
                    int x = g;
                    int y = x;
                }
            "#,
            Some(&test_langspec()),
        )?;
        let semantic = analyze_script(&script, Some(&test_langspec()))?;
        let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))?;

        assert_eq!(hir.globals.len(), 1);
        match hir
            .globals
            .first()
            .and_then(|global| global.initializer.as_ref())
        {
            Some(initializer) => {
                assert_eq!(
                    initializer.kind,
                    HirExprKind::Value(HirValueRef::BuiltinConstant("TRUE".to_string()))
                );
            }
            None => return Err(std::io::Error::other("expected global initializer").into()),
        }

        let main = hir
            .functions
            .iter()
            .find(|function| function.name == "main")
            .ok_or_else(|| std::io::Error::other("main should be lowered"))?;
        assert_eq!(main.locals.len(), 2);
        assert_eq!(
            main.locals.first().map(|local| local.kind),
            Some(HirLocalKind::Local)
        );
        assert_eq!(
            main.locals.get(1).map(|local| local.kind),
            Some(HirLocalKind::Local)
        );

        let body = main
            .body
            .as_ref()
            .ok_or_else(|| std::io::Error::other("main should have a body"))?;

        match body.statements.first() {
            Some(HirStmt::Declare(statement)) => match statement
                .declarators
                .first()
                .and_then(|declarator| declarator.initializer.as_ref())
            {
                Some(initializer) => {
                    assert_eq!(
                        initializer.kind,
                        HirExprKind::Value(HirValueRef::Global("g".to_string()))
                    );
                }
                None => return Err(std::io::Error::other("expected local initializer").into()),
            },
            other => {
                return Err(
                    std::io::Error::other(format!("expected declaration, got {other:?}")).into(),
                );
            }
        }

        match body.statements.get(1) {
            Some(HirStmt::Declare(statement)) => match statement
                .declarators
                .first()
                .and_then(|declarator| declarator.initializer.as_ref())
            {
                Some(initializer) => {
                    let first_local = main
                        .locals
                        .first()
                        .ok_or_else(|| std::io::Error::other("missing first local"))?;
                    assert_eq!(
                        initializer.kind,
                        HirExprKind::Value(HirValueRef::Local(first_local.id))
                    );
                }
                None => return Err(std::io::Error::other("expected local initializer").into()),
            },
            other => {
                return Err(
                    std::io::Error::other(format!("expected declaration, got {other:?}")).into(),
                );
            }
        }
        Ok(())
    }

    #[test]
    fn lowers_builtin_and_user_calls_to_explicit_targets() -> Result<(), Box<dyn std::error::Error>>
    {
        let script = parse_text(
            SourceId::new(71),
            r#"
                void helper() {}
                void main() {
                    helper();
                    DelayCommand(1.0, helper());
                }
            "#,
            Some(&test_langspec()),
        )?;
        let semantic = analyze_script(&script, Some(&test_langspec()))?;
        let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))?;

        let main = hir
            .functions
            .iter()
            .find(|function| function.name == "main")
            .ok_or_else(|| std::io::Error::other("main should be lowered"))?;
        let body = main
            .body
            .as_ref()
            .ok_or_else(|| std::io::Error::other("main should have a body"))?;

        match body.statements.first() {
            Some(HirStmt::Expr(expr)) => match &expr.kind {
                HirExprKind::Call {
                    target, ..
                } => {
                    assert_eq!(target, &HirCallTarget::Function("helper".to_string()));
                }
                other => {
                    return Err(std::io::Error::other(format!(
                        "expected direct call, got {other:?}"
                    ))
                    .into());
                }
            },
            other => {
                return Err(std::io::Error::other(format!(
                    "expected expression statement, got {other:?}"
                ))
                .into());
            }
        }

        match body.statements.get(1) {
            Some(HirStmt::Expr(expr)) => match &expr.kind {
                HirExprKind::Call {
                    target,
                    arguments,
                } => {
                    assert_eq!(target, &HirCallTarget::Builtin("DelayCommand".to_string()));
                    assert_eq!(arguments.len(), 2);
                }
                other => {
                    return Err(std::io::Error::other(format!(
                        "expected builtin call, got {other:?}"
                    ))
                    .into());
                }
            },
            other => {
                return Err(std::io::Error::other(format!(
                    "expected expression statement, got {other:?}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn lowers_function_body_locals_that_shadow_parameter_names()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(72),
            r#"
                int helper(object oSpellTarget) {
                    object oSpellTarget = OBJECT_SELF;
                    return TRUE;
                }
                void main() {}
            "#,
            Some(&test_langspec()),
        )?;
        let semantic = analyze_script(&script, Some(&test_langspec()))?;
        let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))?;

        let helper = hir
            .functions
            .iter()
            .find(|function| function.name == "helper")
            .ok_or_else(|| std::io::Error::other("helper should be lowered"))?;
        assert_eq!(helper.locals.len(), 2);
        assert_eq!(
            helper.locals.first().map(|local| local.kind),
            Some(HirLocalKind::Parameter)
        );
        assert_eq!(
            helper.locals.get(1).map(|local| local.kind),
            Some(HirLocalKind::Local)
        );
        Ok(())
    }
}
