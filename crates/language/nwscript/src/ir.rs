use std::{collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{
    AssignmentOp, BinaryOp, BuiltinType, BuiltinValue, HirBlock, HirCallTarget, HirExpr,
    HirExprKind, HirFunction, HirModule, HirStmt, LangSpec, Literal, SemanticType, UnaryOp,
    nwscript_string_hash,
};

/// One lowered IR module ready for code generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrModule {
    /// Globals lowered into the IR world.
    pub globals:   Vec<IrGlobal>,
    /// Functions lowered into stack-machine IR.
    pub functions: Vec<IrFunction>,
}

/// One lowered global.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrGlobal {
    /// Global name.
    pub name: String,
    /// Global type.
    pub ty:   SemanticType,
}

/// One lowered function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrFunction {
    /// Function name.
    pub name:        String,
    /// Return type.
    pub return_type: SemanticType,
    /// Parameter types in declaration order.
    pub parameters:  Vec<SemanticType>,
    /// Local types by slot.
    pub locals:      Vec<SemanticType>,
    /// Basic blocks in layout order.
    pub blocks:      Vec<IrBlock>,
}

/// One basic block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrBlock {
    /// Block id.
    pub id:           IrBlockId,
    /// Non-terminator instructions.
    pub instructions: Vec<IrInstruction>,
    /// Block terminator.
    pub terminator:   IrTerminator,
}

/// One block id.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct IrBlockId(pub u32);

/// One SSA-like transient value id.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct IrValueId(pub u32);

/// One local slot id.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct IrLocalId(pub u32);

/// One stack-oriented IR instruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IrInstruction {
    /// Materialize one literal.
    Const {
        /// Destination value.
        dst:     IrValueId,
        /// Literal payload.
        literal: Literal,
    },
    /// Load one local.
    LoadLocal {
        /// Destination value.
        dst:   IrValueId,
        /// Local slot.
        local: IrLocalId,
    },
    /// Store one local.
    StoreLocal {
        /// Local slot.
        local: IrLocalId,
        /// Stored value.
        value: IrValueId,
    },
    /// Load one global.
    LoadGlobal {
        /// Destination value.
        dst:  IrValueId,
        /// Global name.
        name: String,
    },
    /// Store one global.
    StoreGlobal {
        /// Global name.
        name:  String,
        /// Stored value.
        value: IrValueId,
    },
    /// Apply one unary operator.
    Unary {
        /// Destination value.
        dst:     IrValueId,
        /// Operator.
        op:      UnaryOp,
        /// Operand.
        operand: IrValueId,
    },
    /// Apply one binary operator.
    Binary {
        /// Destination value.
        dst:   IrValueId,
        /// Operator.
        op:    BinaryOp,
        /// Left operand.
        left:  IrValueId,
        /// Right operand.
        right: IrValueId,
    },
    /// Apply one assignment operator in-place.
    Assignment {
        /// Destination value.
        dst:   IrValueId,
        /// Operator.
        op:    AssignmentOp,
        /// Left operand.
        left:  IrValueId,
        /// Right operand.
        right: IrValueId,
    },
    /// Call one function or builtin by name.
    Call {
        /// Optional return destination.
        dst:       Option<IrValueId>,
        /// Function name.
        function:  String,
        /// Argument payloads in source order.
        arguments: Vec<IrCallArgument>,
    },
    /// Load one structure field.
    FieldLoad {
        /// Destination value.
        dst:   IrValueId,
        /// Base value.
        base:  IrValueId,
        /// Field name.
        field: String,
    },
    /// Store one structure field.
    FieldStore {
        /// Base value.
        base:  IrValueId,
        /// Field name.
        field: String,
        /// Stored value.
        value: IrValueId,
    },
}

/// One control-flow terminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IrTerminator {
    /// Return from the current function.
    Return(Option<IrValueId>),
    /// Unconditional branch.
    Jump(IrBlockId),
    /// Conditional branch.
    Branch {
        /// Condition value.
        condition:  IrValueId,
        /// True target.
        then_block: IrBlockId,
        /// False target.
        else_block: IrBlockId,
    },
    /// Multi-way integer branch.
    Switch {
        /// Condition value.
        condition: IrValueId,
        /// Cases in source order.
        cases:     Vec<(i32, IrBlockId)>,
        /// Default target.
        default:   IrBlockId,
    },
    /// Unreachable control flow.
    Unreachable,
}

/// One lowered call argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IrCallArgument {
    /// One eagerly-evaluated value argument.
    Value(IrValueId),
    /// One deferred action body preserved as HIR because it executes later.
    Action(Box<HirExpr>),
}

/// One HIR-to-IR lowering failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrLowerError {
    /// Optional source span tied to the failure.
    pub span:    Option<crate::Span>,
    /// Human-readable error text.
    pub message: String,
}

impl IrLowerError {
    fn new(span: Option<crate::Span>, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for IrLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for IrLowerError {}

/// Lowers HIR into the compiler IR used by later codegen work.
pub fn lower_hir_to_ir(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
) -> Result<IrModule, IrLowerError> {
    IrLowerer::new(hir, langspec).lower()
}

struct IrLowerer<'a> {
    hir:                     &'a HirModule,
    builtin_constants:       BTreeMap<String, Literal>,
    builtin_parameter_types: BTreeMap<String, Vec<SemanticType>>,
    functions:               BTreeMap<String, &'a HirFunction>,
}

impl<'a> IrLowerer<'a> {
    fn new(hir: &'a HirModule, langspec: Option<&LangSpec>) -> Self {
        let mut builtin_constants = BTreeMap::new();
        let mut builtin_parameter_types = BTreeMap::new();
        if let Some(langspec) = langspec {
            for constant in &langspec.constants {
                if let Some(literal) = literal_from_builtin_value(&constant.value) {
                    builtin_constants.insert(constant.name.clone(), literal);
                }
            }
            for function in &langspec.functions {
                builtin_parameter_types.insert(
                    function.name.clone(),
                    function
                        .parameters
                        .iter()
                        .map(|parameter| semantic_type_from_builtin_type(&parameter.ty))
                        .collect(),
                );
            }
        }
        let functions = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), function))
            .collect::<BTreeMap<_, _>>();
        Self {
            hir,
            builtin_constants,
            builtin_parameter_types,
            functions,
        }
    }

    fn lower(self) -> Result<IrModule, IrLowerError> {
        let globals = self
            .hir
            .globals
            .iter()
            .map(|global| IrGlobal {
                name: global.name.clone(),
                ty:   global.ty.clone(),
            })
            .collect();
        let mut functions = Vec::new();
        for function in &self.hir.functions {
            if function.is_builtin {
                continue;
            }
            functions.push(FunctionLowerer::new(&self, function).lower()?);
        }
        Ok(IrModule {
            globals,
            functions,
        })
    }
}

struct BlockBuilder {
    id:           IrBlockId,
    instructions: Vec<IrInstruction>,
    terminator:   Option<IrTerminator>,
}

struct FunctionLowerer<'a, 'b> {
    lowerer:          &'b IrLowerer<'a>,
    function:         &'a HirFunction,
    locals:           Vec<SemanticType>,
    blocks:           Vec<BlockBuilder>,
    next_value:       u32,
    break_targets:    Vec<IrBlockId>,
    continue_targets: Vec<IrBlockId>,
}

impl<'a, 'b> FunctionLowerer<'a, 'b> {
    fn new(lowerer: &'b IrLowerer<'a>, function: &'a HirFunction) -> Self {
        Self {
            lowerer,
            function,
            locals: function
                .locals
                .iter()
                .map(|local| local.ty.clone())
                .collect(),
            blocks: Vec::new(),
            next_value: 0,
            break_targets: Vec::new(),
            continue_targets: Vec::new(),
        }
    }

    fn lower(mut self) -> Result<IrFunction, IrLowerError> {
        if let Some(body) = &self.function.body {
            let entry = self.new_block();
            let tail = self.lower_block(body, Some(entry))?;
            if let Some(tail) = tail {
                let terminator = if self.function.return_type == SemanticType::Void {
                    IrTerminator::Return(None)
                } else {
                    IrTerminator::Unreachable
                };
                self.set_terminator(tail, terminator)?;
            }
        }

        Ok(IrFunction {
            name:        self.function.name.clone(),
            return_type: self.function.return_type.clone(),
            parameters:  self
                .function
                .parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect(),
            locals:      self.locals,
            blocks:      self
                .blocks
                .into_iter()
                .map(|block| IrBlock {
                    id:           block.id,
                    instructions: block.instructions,
                    terminator:   block.terminator.unwrap_or(IrTerminator::Unreachable),
                })
                .collect(),
        })
    }

    fn new_block(&mut self) -> IrBlockId {
        let id = IrBlockId(u32::try_from(self.blocks.len()).ok().unwrap_or(u32::MAX));
        self.blocks.push(BlockBuilder {
            id,
            instructions: Vec::new(),
            terminator: None,
        });
        id
    }

    fn block_mut(&mut self, id: IrBlockId) -> Result<&mut BlockBuilder, IrLowerError> {
        self.blocks
            .get_mut(id.0 as usize)
            .ok_or_else(|| IrLowerError::new(None, format!("unknown IR block {:?}", id)))
    }

    fn push_instruction(
        &mut self,
        block: IrBlockId,
        instruction: IrInstruction,
    ) -> Result<(), IrLowerError> {
        self.block_mut(block)?.instructions.push(instruction);
        Ok(())
    }

    fn set_terminator(
        &mut self,
        block: IrBlockId,
        terminator: IrTerminator,
    ) -> Result<(), IrLowerError> {
        self.block_mut(block)?.terminator = Some(terminator);
        Ok(())
    }

    fn new_value(&mut self) -> IrValueId {
        let id = IrValueId(self.next_value);
        self.next_value += 1;
        id
    }

    fn new_temp_local(&mut self, ty: SemanticType) -> IrLocalId {
        let local = IrLocalId(u32::try_from(self.locals.len()).ok().unwrap_or(u32::MAX));
        self.locals.push(ty);
        local
    }

    fn lower_block(
        &mut self,
        block: &HirBlock,
        mut current: Option<IrBlockId>,
    ) -> Result<Option<IrBlockId>, IrLowerError> {
        for statement in &block.statements {
            current = self.lower_stmt(statement, current)?;
        }
        Ok(current)
    }

    fn lower_stmt(
        &mut self,
        statement: &HirStmt,
        current: Option<IrBlockId>,
    ) -> Result<Option<IrBlockId>, IrLowerError> {
        let Some(current) = current else {
            return Ok(None);
        };

        match statement {
            HirStmt::Block(block) => self.lower_block(block, Some(current)),
            HirStmt::Declare(statement) => {
                let mut current = current;
                for declarator in &statement.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        let (value, next_block) = self.lower_expr(initializer, current)?;
                        current = next_block;
                        let value = value.ok_or_else(|| {
                            IrLowerError::new(
                                Some(initializer.span),
                                "void initializer is not supported in IR",
                            )
                        })?;
                        self.push_instruction(
                            current,
                            IrInstruction::StoreLocal {
                                local: IrLocalId(declarator.local.0),
                                value,
                            },
                        )?;
                    }
                }
                Ok(Some(current))
            }
            HirStmt::Expr(expr) => {
                let (_value, current) = self.lower_expr(expr, current)?;
                Ok(Some(current))
            }
            HirStmt::If(statement) => self.lower_if(statement, current),
            HirStmt::Switch(statement) => self.lower_switch(statement, current),
            HirStmt::Return(statement) => {
                let value = statement
                    .value
                    .as_ref()
                    .map(|expr| self.lower_expr(expr, current))
                    .transpose()?;
                let (value, current) = value.unwrap_or((None, current));
                self.set_terminator(current, IrTerminator::Return(value))?;
                Ok(None)
            }
            HirStmt::While(statement) => self.lower_while(statement, current),
            HirStmt::DoWhile(statement) => self.lower_do_while(statement, current),
            HirStmt::For(statement) => self.lower_for(statement, current),
            HirStmt::Case(_) | HirStmt::Default(_) => Err(IrLowerError::new(
                None,
                "case/default labels must be lowered through lower_switch",
            )),
            HirStmt::Break(span) => {
                let target = self.break_targets.last().copied().ok_or_else(|| {
                    IrLowerError::new(Some(*span), "break used outside loop or switch")
                })?;
                self.set_terminator(current, IrTerminator::Jump(target))?;
                Ok(None)
            }
            HirStmt::Continue(span) => {
                let target =
                    self.continue_targets.last().copied().ok_or_else(|| {
                        IrLowerError::new(Some(*span), "continue used outside loop")
                    })?;
                self.set_terminator(current, IrTerminator::Jump(target))?;
                Ok(None)
            }
            HirStmt::Empty(_) => Ok(Some(current)),
        }
    }

    fn lower_if(
        &mut self,
        statement: &crate::HirIfStmt,
        current: IrBlockId,
    ) -> Result<Option<IrBlockId>, IrLowerError> {
        let (condition, current) = self.lower_expr(&statement.condition, current)?;
        let condition = condition.ok_or_else(|| {
            IrLowerError::new(
                Some(statement.condition.span),
                "if condition must produce a value",
            )
        })?;
        let then_block = self.new_block();
        let else_block = self.new_block();
        self.set_terminator(
            current,
            IrTerminator::Branch {
                condition,
                then_block,
                else_block,
            },
        )?;

        let then_tail = self.lower_stmt(&statement.then_branch, Some(then_block))?;
        let else_tail = if let Some(else_branch) = &statement.else_branch {
            self.lower_stmt(else_branch, Some(else_block))?
        } else {
            Some(else_block)
        };

        let join = self.new_block();
        let mut falls_through = false;
        if let Some(then_tail) = then_tail {
            self.set_terminator(then_tail, IrTerminator::Jump(join))?;
            falls_through = true;
        }
        if let Some(else_tail) = else_tail {
            self.set_terminator(else_tail, IrTerminator::Jump(join))?;
            falls_through = true;
        }

        Ok(falls_through.then_some(join))
    }

    fn lower_while(
        &mut self,
        statement: &crate::HirWhileStmt,
        current: IrBlockId,
    ) -> Result<Option<IrBlockId>, IrLowerError> {
        let cond_block = self.new_block();
        let body_block = self.new_block();
        let end_block = self.new_block();
        self.set_terminator(current, IrTerminator::Jump(cond_block))?;

        let (condition, cond_block) = self.lower_expr(&statement.condition, cond_block)?;
        let condition = condition.ok_or_else(|| {
            IrLowerError::new(
                Some(statement.condition.span),
                "while condition must produce a value",
            )
        })?;
        self.set_terminator(
            cond_block,
            IrTerminator::Branch {
                condition,
                then_block: body_block,
                else_block: end_block,
            },
        )?;

        self.break_targets.push(end_block);
        self.continue_targets.push(cond_block);
        let body_tail = self.lower_stmt(&statement.body, Some(body_block))?;
        self.continue_targets.pop();
        self.break_targets.pop();
        if let Some(body_tail) = body_tail {
            self.set_terminator(body_tail, IrTerminator::Jump(cond_block))?;
        }

        Ok(Some(end_block))
    }

    fn lower_do_while(
        &mut self,
        statement: &crate::HirDoWhileStmt,
        current: IrBlockId,
    ) -> Result<Option<IrBlockId>, IrLowerError> {
        let body_block = self.new_block();
        let cond_block = self.new_block();
        let end_block = self.new_block();
        self.set_terminator(current, IrTerminator::Jump(body_block))?;

        self.break_targets.push(end_block);
        self.continue_targets.push(cond_block);
        let body_tail = self.lower_stmt(&statement.body, Some(body_block))?;
        self.continue_targets.pop();
        self.break_targets.pop();
        if let Some(body_tail) = body_tail {
            self.set_terminator(body_tail, IrTerminator::Jump(cond_block))?;
        }

        let (condition, cond_block) = self.lower_expr(&statement.condition, cond_block)?;
        let condition = condition.ok_or_else(|| {
            IrLowerError::new(
                Some(statement.condition.span),
                "do/while condition must produce a value",
            )
        })?;
        self.set_terminator(
            cond_block,
            IrTerminator::Branch {
                condition,
                then_block: body_block,
                else_block: end_block,
            },
        )?;

        Ok(Some(end_block))
    }

    fn lower_for(
        &mut self,
        statement: &crate::HirForStmt,
        current: IrBlockId,
    ) -> Result<Option<IrBlockId>, IrLowerError> {
        let mut current = current;
        if let Some(initializer) = &statement.initializer {
            let (_value, next_block) = self.lower_expr(initializer, current)?;
            current = next_block;
        }

        let cond_block = self.new_block();
        let body_block = self.new_block();
        let update_block = self.new_block();
        let end_block = self.new_block();
        self.set_terminator(current, IrTerminator::Jump(cond_block))?;

        if let Some(condition_expr) = &statement.condition {
            let (condition, cond_block) = self.lower_expr(condition_expr, cond_block)?;
            let condition = condition.ok_or_else(|| {
                IrLowerError::new(
                    Some(condition_expr.span),
                    "for condition must produce a value",
                )
            })?;
            self.set_terminator(
                cond_block,
                IrTerminator::Branch {
                    condition,
                    then_block: body_block,
                    else_block: end_block,
                },
            )?;
        } else {
            self.set_terminator(cond_block, IrTerminator::Jump(body_block))?;
        }

        self.break_targets.push(end_block);
        self.continue_targets.push(update_block);
        let body_tail = self.lower_stmt(&statement.body, Some(body_block))?;
        self.continue_targets.pop();
        self.break_targets.pop();
        if let Some(body_tail) = body_tail {
            self.set_terminator(body_tail, IrTerminator::Jump(update_block))?;
        }

        if let Some(update) = &statement.update {
            let (_value, update_block) = self.lower_expr(update, update_block)?;
            self.set_terminator(update_block, IrTerminator::Jump(cond_block))?;
            return Ok(Some(end_block));
        }
        self.set_terminator(update_block, IrTerminator::Jump(cond_block))?;

        Ok(Some(end_block))
    }

    fn lower_switch(
        &mut self,
        statement: &crate::HirSwitchStmt,
        current: IrBlockId,
    ) -> Result<Option<IrBlockId>, IrLowerError> {
        let HirStmt::Block(block) = statement.body.as_ref() else {
            return Err(IrLowerError::new(
                Some(statement.span),
                "switch lowering requires a block body",
            ));
        };
        let (condition, current) = self.lower_expr(&statement.condition, current)?;
        let condition = condition.ok_or_else(|| {
            IrLowerError::new(
                Some(statement.condition.span),
                "switch condition must produce a value",
            )
        })?;
        let end_block = self.new_block();

        let mut case_targets = Vec::new();
        let mut default_target = end_block;
        for stmt in &block.statements {
            match stmt {
                HirStmt::Case(expr) => case_targets.push((
                    evaluate_case_value(expr, &self.lowerer.builtin_constants)?,
                    self.new_block(),
                )),
                HirStmt::Default(_) => {
                    default_target = self.new_block();
                }
                _ => {}
            }
        }
        self.set_terminator(
            current,
            IrTerminator::Switch {
                condition,
                cases: case_targets.clone(),
                default: default_target,
            },
        )?;

        self.break_targets.push(end_block);
        let mut active: Option<IrBlockId> = None;
        let mut next_case = 0usize;
        for stmt in &block.statements {
            match stmt {
                HirStmt::Case(_) => {
                    let Some((_, target)) = case_targets.get(next_case).copied() else {
                        return Err(IrLowerError::new(
                            Some(statement.span),
                            "case label index out of bounds during IR lowering",
                        ));
                    };
                    next_case += 1;
                    if let Some(active_block) = active {
                        self.set_terminator(active_block, IrTerminator::Jump(target))?;
                    }
                    active = Some(target);
                }
                HirStmt::Default(_) => {
                    if let Some(active_block) = active {
                        self.set_terminator(active_block, IrTerminator::Jump(default_target))?;
                    }
                    active = Some(default_target);
                }
                other => {
                    let current = active.ok_or_else(|| {
                        IrLowerError::new(
                            Some(statement.span),
                            "switch body contained statements before any case/default label",
                        )
                    })?;
                    active = self.lower_stmt(other, Some(current))?;
                }
            }
        }
        self.break_targets.pop();

        if let Some(active) = active {
            self.set_terminator(active, IrTerminator::Jump(end_block))?;
        }

        Ok(Some(end_block))
    }

    fn lower_expr(
        &mut self,
        expr: &HirExpr,
        block: IrBlockId,
    ) -> Result<(Option<IrValueId>, IrBlockId), IrLowerError> {
        match &expr.kind {
            HirExprKind::Literal(literal) => {
                let dst = self.new_value();
                self.push_instruction(
                    block,
                    IrInstruction::Const {
                        dst,
                        literal: literal.clone(),
                    },
                )?;
                Ok((Some(dst), block))
            }
            HirExprKind::Value(crate::HirValueRef::Local(local)) => {
                let dst = self.new_value();
                self.push_instruction(
                    block,
                    IrInstruction::LoadLocal {
                        dst,
                        local: IrLocalId(local.0),
                    },
                )?;
                Ok((Some(dst), block))
            }
            HirExprKind::Value(crate::HirValueRef::Global(name))
            | HirExprKind::Value(crate::HirValueRef::ConstGlobal(name)) => {
                let dst = self.new_value();
                self.push_instruction(
                    block,
                    IrInstruction::LoadGlobal {
                        dst,
                        name: name.clone(),
                    },
                )?;
                Ok((Some(dst), block))
            }
            HirExprKind::Value(crate::HirValueRef::BuiltinConstant(name)) => {
                let literal = self.lowerer.builtin_constants.get(name).ok_or_else(|| {
                    IrLowerError::new(
                        Some(expr.span),
                        format!("unknown builtin constant {:?}", name),
                    )
                })?;
                let dst = self.new_value();
                self.push_instruction(
                    block,
                    IrInstruction::Const {
                        dst,
                        literal: literal.clone(),
                    },
                )?;
                Ok((Some(dst), block))
            }
            HirExprKind::Call {
                target,
                arguments,
            } => {
                let function_name = match target {
                    HirCallTarget::Builtin(name) | HirCallTarget::Function(name) => name.clone(),
                };
                let mut current = block;
                let mut lowered_arguments = Vec::new();
                match target {
                    HirCallTarget::Builtin(_) => {
                        let parameter_types = self
                            .lowerer
                            .builtin_parameter_types
                            .get(&function_name)
                            .ok_or_else(|| {
                                IrLowerError::new(
                                    Some(expr.span),
                                    format!("unknown builtin {:?}", function_name),
                                )
                            })?;
                        for (index, argument) in arguments.iter().enumerate() {
                            if parameter_types
                                .get(index)
                                .is_some_and(|ty| *ty == SemanticType::Action)
                            {
                                lowered_arguments
                                    .push(IrCallArgument::Action(Box::new(argument.clone())));
                                continue;
                            }

                            let (value, next_block) = self.lower_expr(argument, current)?;
                            current = next_block;
                            let value = value.ok_or_else(|| {
                                IrLowerError::new(
                                    Some(argument.span),
                                    "void-valued call arguments are not represented in IR yet",
                                )
                            })?;
                            lowered_arguments.push(IrCallArgument::Value(value));
                        }
                    }
                    HirCallTarget::Function(name) => {
                        let callee = self.lowerer.functions.get(name).ok_or_else(|| {
                            IrLowerError::new(
                                Some(expr.span),
                                format!("unknown function {:?}", name),
                            )
                        })?;
                        for (argument, parameter) in arguments.iter().zip(&callee.parameters) {
                            if parameter.ty == SemanticType::Action {
                                lowered_arguments
                                    .push(IrCallArgument::Action(Box::new(argument.clone())));
                                continue;
                            }

                            let (value, next_block) = self.lower_expr(argument, current)?;
                            current = next_block;
                            let value = value.ok_or_else(|| {
                                IrLowerError::new(
                                    Some(argument.span),
                                    "void-valued call arguments are not represented in IR yet",
                                )
                            })?;
                            lowered_arguments.push(IrCallArgument::Value(value));
                        }

                        if arguments.len() < callee.parameters.len() {
                            for parameter in callee.parameters.iter().skip(arguments.len()) {
                                let default = parameter.default.as_ref().ok_or_else(|| {
                                    IrLowerError::new(
                                        Some(expr.span),
                                        format!(
                                            "missing required parameter for function {:?}",
                                            name
                                        ),
                                    )
                                })?;
                                if parameter.ty == SemanticType::Action {
                                    lowered_arguments
                                        .push(IrCallArgument::Action(Box::new(default.clone())));
                                    continue;
                                }

                                let (value, next_block) = self.lower_expr(default, current)?;
                                current = next_block;
                                let value = value.ok_or_else(|| {
                                    IrLowerError::new(
                                        Some(default.span),
                                        "void-valued default argument is not supported in IR",
                                    )
                                })?;
                                lowered_arguments.push(IrCallArgument::Value(value));
                            }
                        }
                    }
                }

                let dst = if expr.ty == SemanticType::Void {
                    None
                } else {
                    Some(self.new_value())
                };
                self.push_instruction(
                    current,
                    IrInstruction::Call {
                        dst,
                        function: function_name,
                        arguments: lowered_arguments,
                    },
                )?;
                Ok((dst, current))
            }
            HirExprKind::FieldAccess {
                base,
                field,
            } => {
                let (base, block) = self.lower_expr(base, block)?;
                let base = base.ok_or_else(|| {
                    IrLowerError::new(
                        Some(expr.span),
                        "field access requires a value-producing base",
                    )
                })?;
                let dst = self.new_value();
                self.push_instruction(
                    block,
                    IrInstruction::FieldLoad {
                        dst,
                        base,
                        field: field.clone(),
                    },
                )?;
                Ok((Some(dst), block))
            }
            HirExprKind::Unary {
                op,
                expr: inner,
            } => match op {
                UnaryOp::PreIncrement
                | UnaryOp::PreDecrement
                | UnaryOp::PostIncrement
                | UnaryOp::PostDecrement => {
                    let (old, block) = self.lower_expr(inner, block)?;
                    let old = old.ok_or_else(|| {
                        IrLowerError::new(Some(inner.span), "increment requires an int lvalue")
                    })?;
                    let one = self.new_value();
                    self.push_instruction(
                        block,
                        IrInstruction::Const {
                            dst:     one,
                            literal: Literal::Integer(1),
                        },
                    )?;
                    let next = self.new_value();
                    self.push_instruction(
                        block,
                        IrInstruction::Binary {
                            dst:   next,
                            op:    match op {
                                UnaryOp::PreIncrement | UnaryOp::PostIncrement => BinaryOp::Add,
                                UnaryOp::PreDecrement | UnaryOp::PostDecrement => {
                                    BinaryOp::Subtract
                                }
                                _ => unreachable!(),
                            },
                            left:  old,
                            right: one,
                        },
                    )?;
                    self.lower_store_target(inner, block, next)?;
                    Ok((
                        if matches!(op, UnaryOp::PostIncrement | UnaryOp::PostDecrement) {
                            Some(old)
                        } else {
                            Some(next)
                        },
                        block,
                    ))
                }
                _ => {
                    let (operand, block) = self.lower_expr(inner, block)?;
                    let operand = operand.ok_or_else(|| {
                        IrLowerError::new(Some(inner.span), "unary operator requires a value")
                    })?;
                    let dst = self.new_value();
                    self.push_instruction(
                        block,
                        IrInstruction::Unary {
                            dst,
                            op: *op,
                            operand,
                        },
                    )?;
                    Ok((Some(dst), block))
                }
            },
            HirExprKind::Binary {
                op,
                left,
                right,
            } => {
                let (left_value, block) = self.lower_expr(left, block)?;
                let left_value = left_value.ok_or_else(|| {
                    IrLowerError::new(Some(left.span), "left operand must produce a value")
                })?;
                let (right_value, block) = self.lower_expr(right, block)?;
                let right_value = right_value.ok_or_else(|| {
                    IrLowerError::new(Some(right.span), "right operand must produce a value")
                })?;
                let dst = self.new_value();
                self.push_instruction(
                    block,
                    IrInstruction::Binary {
                        dst,
                        op: *op,
                        left: left_value,
                        right: right_value,
                    },
                )?;
                Ok((Some(dst), block))
            }
            HirExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                let (condition_value, block) = self.lower_expr(condition, block)?;
                let condition_value = condition_value.ok_or_else(|| {
                    IrLowerError::new(
                        Some(condition.span),
                        "conditional expression condition must produce a value",
                    )
                })?;
                let then_block = self.new_block();
                let else_block = self.new_block();
                let join_block = self.new_block();
                self.set_terminator(
                    block,
                    IrTerminator::Branch {
                        condition: condition_value,
                        then_block,
                        else_block,
                    },
                )?;

                let result_local =
                    (expr.ty != SemanticType::Void).then(|| self.new_temp_local(expr.ty.clone()));

                let (then_value, then_tail) = self.lower_expr(when_true, then_block)?;
                if let Some(local) = result_local {
                    let then_value = then_value.ok_or_else(|| {
                        IrLowerError::new(
                            Some(when_true.span),
                            "conditional true branch must produce a value",
                        )
                    })?;
                    self.push_instruction(
                        then_tail,
                        IrInstruction::StoreLocal {
                            local,
                            value: then_value,
                        },
                    )?;
                }
                self.set_terminator(then_tail, IrTerminator::Jump(join_block))?;

                let (else_value, else_tail) = self.lower_expr(when_false, else_block)?;
                if let Some(local) = result_local {
                    let else_value = else_value.ok_or_else(|| {
                        IrLowerError::new(
                            Some(when_false.span),
                            "conditional false branch must produce a value",
                        )
                    })?;
                    self.push_instruction(
                        else_tail,
                        IrInstruction::StoreLocal {
                            local,
                            value: else_value,
                        },
                    )?;
                }
                self.set_terminator(else_tail, IrTerminator::Jump(join_block))?;

                if let Some(local) = result_local {
                    let dst = self.new_value();
                    self.push_instruction(
                        join_block,
                        IrInstruction::LoadLocal {
                            dst,
                            local,
                        },
                    )?;
                    Ok((Some(dst), join_block))
                } else {
                    Ok((None, join_block))
                }
            }
            HirExprKind::Assignment {
                op,
                left,
                right,
            } => {
                if *op == AssignmentOp::Assign {
                    let (value, block) = self.lower_expr(right, block)?;
                    let value = value.ok_or_else(|| {
                        IrLowerError::new(Some(right.span), "assignment requires a value")
                    })?;
                    self.lower_store_target(left, block, value)?;
                    return Ok((Some(value), block));
                }

                let (left_value, block) = self.lower_expr(left, block)?;
                let left_value = left_value.ok_or_else(|| {
                    IrLowerError::new(Some(left.span), "assignment target must produce a value")
                })?;
                let (right_value, block) = self.lower_expr(right, block)?;
                let right_value = right_value.ok_or_else(|| {
                    IrLowerError::new(Some(right.span), "assignment requires a value")
                })?;
                let dst = self.new_value();
                self.push_instruction(
                    block,
                    IrInstruction::Assignment {
                        dst,
                        op: *op,
                        left: left_value,
                        right: right_value,
                    },
                )?;
                self.lower_store_target(left, block, dst)?;
                Ok((Some(dst), block))
            }
        }
    }

    fn lower_store_target(
        &mut self,
        target: &HirExpr,
        block: IrBlockId,
        value: IrValueId,
    ) -> Result<(), IrLowerError> {
        match &target.kind {
            HirExprKind::Value(crate::HirValueRef::Local(local)) => self.push_instruction(
                block,
                IrInstruction::StoreLocal {
                    local: IrLocalId(local.0),
                    value,
                },
            ),
            HirExprKind::Value(crate::HirValueRef::Global(name))
            | HirExprKind::Value(crate::HirValueRef::ConstGlobal(name)) => self.push_instruction(
                block,
                IrInstruction::StoreGlobal {
                    name: name.clone(),
                    value,
                },
            ),
            _ => Err(IrLowerError::new(
                Some(target.span),
                "IR lowering only supports local/global assignment targets",
            )),
        }
    }
}

fn evaluate_case_value(
    expr: &HirExpr,
    builtin_constants: &BTreeMap<String, Literal>,
) -> Result<i32, IrLowerError> {
    match &expr.kind {
        HirExprKind::Literal(Literal::Integer(value)) => Ok(*value),
        HirExprKind::Literal(Literal::String(value)) => Ok(nwscript_string_hash(value)),
        HirExprKind::Value(crate::HirValueRef::BuiltinConstant(name)) => {
            let literal = builtin_constants.get(name).ok_or_else(|| {
                IrLowerError::new(
                    Some(expr.span),
                    format!("unknown builtin constant {:?}", name),
                )
            })?;
            match literal {
                Literal::Integer(value) => Ok(*value),
                Literal::String(value) => Ok(nwscript_string_hash(value)),
                _ => Err(IrLowerError::new(
                    Some(expr.span),
                    "switch case requires an int or string constant",
                )),
            }
        }
        _ => Err(IrLowerError::new(
            Some(expr.span),
            "switch case requires a constant int or string",
        )),
    }
}

fn literal_from_builtin_value(value: &BuiltinValue) -> Option<Literal> {
    match value {
        BuiltinValue::Int(value) => Some(Literal::Integer(*value)),
        BuiltinValue::Float(value) => Some(Literal::Float(*value)),
        BuiltinValue::String(value) => Some(Literal::String(value.clone())),
        BuiltinValue::ObjectId(value) => Some(Literal::Integer(*value)),
        BuiltinValue::ObjectSelf => Some(Literal::ObjectSelf),
        BuiltinValue::ObjectInvalid => Some(Literal::ObjectInvalid),
        BuiltinValue::LocationInvalid => Some(Literal::LocationInvalid),
        BuiltinValue::Json(value) => Some(Literal::Json(value.clone())),
        BuiltinValue::Vector(value) => Some(Literal::Vector(*value)),
        BuiltinValue::Raw(_) => None,
    }
}

fn semantic_type_from_builtin_type(ty: &BuiltinType) -> SemanticType {
    match ty {
        BuiltinType::Void => SemanticType::Void,
        BuiltinType::Int => SemanticType::Int,
        BuiltinType::Float => SemanticType::Float,
        BuiltinType::String => SemanticType::String,
        BuiltinType::Object => SemanticType::Object,
        BuiltinType::Action => SemanticType::Action,
        BuiltinType::Vector => SemanticType::Vector,
        BuiltinType::EngineStructure(name) => SemanticType::EngineStructure(name.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::{IrCallArgument, IrInstruction, IrTerminator, lower_hir_to_ir};
    use crate::{
        BuiltinConstant, BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue,
        HirCallTarget, HirExprKind, LangSpec, SourceId, analyze_script, lower_to_hir, parse_text,
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
                    name:  "FALSE".to_string(),
                    ty:    BuiltinType::Int,
                    value: BuiltinValue::Int(0),
                },
            ],
            functions:             vec![
                BuiltinFunction {
                    name:        "GetCurrentHitPoints".to_string(),
                    return_type: BuiltinType::Int,
                    parameters:  vec![],
                },
                BuiltinFunction {
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
                },
            ],
        }
    }

    #[test]
    fn lowers_simple_control_flow_to_basic_blocks() {
        let script = parse_text(
            SourceId::new(90),
            r#"
                int StartingConditional() {
                    int nCurHP = GetCurrentHitPoints();
                    if (nCurHP > 0) {
                        return TRUE;
                    }
                    return FALSE;
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let semantic =
            analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
        let hir =
            lower_to_hir(&script, &semantic, Some(&test_langspec())).expect("HIR should lower");
        let ir = lower_hir_to_ir(&hir, Some(&test_langspec())).expect("IR should lower");

        let function = ir
            .functions
            .iter()
            .find(|function| function.name == "StartingConditional")
            .expect("function should exist");

        assert!(function.blocks.len() >= 4);
        assert!(function.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(instruction, IrInstruction::Call { function, .. } if function == "GetCurrentHitPoints")
            })
        }));
        assert!(
            function
                .blocks
                .iter()
                .any(|block| matches!(block.terminator, IrTerminator::Branch { .. }))
        );
    }

    #[test]
    fn lowers_user_optional_parameter_defaults_into_call_arguments() {
        let script = parse_text(
            SourceId::new(91),
            r#"
                int AddOne(int nBase = TRUE) {
                    return nBase + 1;
                }
                int StartingConditional() {
                    return AddOne();
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let semantic =
            analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
        let hir =
            lower_to_hir(&script, &semantic, Some(&test_langspec())).expect("HIR should lower");
        let ir = lower_hir_to_ir(&hir, Some(&test_langspec())).expect("IR should lower");

        let caller = ir
            .functions
            .iter()
            .find(|function| function.name == "StartingConditional")
            .expect("caller should exist");
        assert!(caller.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(instruction, IrInstruction::Call { function, arguments, .. } if function == "AddOne" && arguments.len() == 1)
            })
        }));
    }

    #[test]
    fn lowers_conditional_expressions_into_branching_ir() {
        let script = parse_text(
            SourceId::new(92),
            r#"
                int StartingConditional() {
                    int nCurHP = GetCurrentHitPoints();
                    return nCurHP > 0 ? TRUE : FALSE;
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let semantic =
            analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
        let hir =
            lower_to_hir(&script, &semantic, Some(&test_langspec())).expect("HIR should lower");
        let ir = lower_hir_to_ir(&hir, Some(&test_langspec())).expect("IR should lower");

        let function = ir
            .functions
            .iter()
            .find(|function| function.name == "StartingConditional")
            .expect("function should exist");

        assert!(
            function
                .blocks
                .iter()
                .any(|block| matches!(block.terminator, IrTerminator::Branch { .. })),
            "conditional expression should lower into a branch",
        );
        assert!(
            function.blocks.iter().any(|block| {
                block
                    .instructions
                    .iter()
                    .any(|instruction| matches!(instruction, IrInstruction::StoreLocal { .. }))
            }),
            "conditional expression should store branch results into a merge slot",
        );
        assert!(
            function.blocks.iter().any(|block| {
                block
                    .instructions
                    .iter()
                    .any(|instruction| matches!(instruction, IrInstruction::LoadLocal { .. }))
            }),
            "conditional expression should reload the merged branch result",
        );
    }

    #[test]
    fn preserves_deferred_action_arguments_in_ir_calls() {
        let script = parse_text(
            SourceId::new(93),
            r#"
                void helper() {}
                void main() {
                    DelayCommand(1.0, helper());
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let semantic =
            analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
        let hir =
            lower_to_hir(&script, &semantic, Some(&test_langspec())).expect("HIR should lower");
        let ir = lower_hir_to_ir(&hir, Some(&test_langspec())).expect("IR should lower");

        let main = ir
            .functions
            .iter()
            .find(|function| function.name == "main")
            .expect("main should exist");
        let call = main
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find_map(|instruction| match instruction {
                IrInstruction::Call {
                    function,
                    arguments,
                    ..
                } if function == "DelayCommand" => Some(arguments),
                _ => None,
            })
            .expect("DelayCommand call should be present");

        assert_eq!(call.len(), 2);
        assert!(matches!(call.first(), Some(IrCallArgument::Value(_))));
        assert!(matches!(call.get(1), Some(IrCallArgument::Action(_))));
        let action = match call.get(1) {
            Some(IrCallArgument::Action(action)) => action,
            _ => return,
        };
        assert!(matches!(&action.kind, HirExprKind::Call { .. }));
        let (target, arguments) = match &action.kind {
            HirExprKind::Call {
                target,
                arguments,
            } => (target, arguments),
            _ => return,
        };
        assert_eq!(target, &HirCallTarget::Function("helper".to_string()));
        assert!(arguments.is_empty());
    }
}
