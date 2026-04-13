use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use serde::{Deserialize, Serialize};

use crate::{
    AssignmentOp, BinaryOp, BlockStmt, BuiltinType, BuiltinValue, CompilerErrorCode, Expr,
    ExprKind, FunctionDecl, LangSpec, Literal, MagicLiteral, Script, Stmt, TopLevelItem, TypeKind,
    TypeSpec, UnaryOp, nwscript_string_hash,
};

/// Options controlling semantic analysis checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct SemanticOptions {
    /// Require a valid `main()` or `StartingConditional()` entry point.
    pub require_entrypoint:       bool,
    /// Permit `StartingConditional()` as the required entry point when `main()`
    /// is not present.
    pub allow_conditional_script: bool,
}

/// One semantic-analysis error aligned to the upstream compiler's diagnostic
/// space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticError {
    /// Stable upstream-aligned compiler error code.
    pub code:    CompilerErrorCode,
    /// Source span where semantic analysis failed.
    pub span:    crate::Span,
    /// Human-readable error message.
    pub message: String,
}

impl SemanticError {
    fn new(code: CompilerErrorCode, span: crate::Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code.code())
    }
}

impl Error for SemanticError {}

/// One resolved semantic type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticType {
    /// `void`
    Void,
    /// `int`
    Int,
    /// `float`
    Float,
    /// `string`
    String,
    /// `object`
    Object,
    /// `action`
    Action,
    /// `vector`
    Vector,
    /// One user-defined structure.
    Struct(String),
    /// One engine-defined structure such as `effect` or `json`.
    EngineStructure(String),
}

/// One resolved function parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticParameter {
    /// Parameter name.
    pub name:        String,
    /// Resolved parameter type.
    pub ty:          SemanticType,
    /// Whether the parameter has a default value.
    pub is_optional: bool,
    /// Folded default value for omitted trailing arguments.
    pub default:     Option<Literal>,
}

/// One resolved function signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticFunction {
    /// Function name.
    pub name:        String,
    /// Resolved return type.
    pub return_type: SemanticType,
    /// Parameters in declaration order.
    pub parameters:  Vec<SemanticParameter>,
    /// Whether this function has a body in the current script.
    pub has_body:    bool,
    /// Whether this function came from `nwscript.nss`.
    pub is_builtin:  bool,
}

/// One resolved global variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticGlobal {
    /// Variable name.
    pub name:     String,
    /// Resolved type.
    pub ty:       SemanticType,
    /// Whether the declaration used `const`.
    pub is_const: bool,
}

/// One resolved structure field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticField {
    /// Field name.
    pub name: String,
    /// Resolved field type.
    pub ty:   SemanticType,
}

/// One resolved user-defined structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticStruct {
    /// Structure name.
    pub name:   String,
    /// Fields in declaration order.
    pub fields: Vec<SemanticField>,
}

/// Semantic facts collected from one script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticModel {
    /// Resolved structures indexed by name.
    pub structs:   BTreeMap<String, SemanticStruct>,
    /// Resolved global variables indexed by name.
    pub globals:   BTreeMap<String, SemanticGlobal>,
    /// Resolved functions indexed by name.
    pub functions: BTreeMap<String, SemanticFunction>,
}

/// Performs semantic analysis on one parsed script.
pub fn analyze_script(
    script: &Script,
    langspec: Option<&LangSpec>,
) -> Result<SemanticModel, SemanticError> {
    analyze_script_with_options(script, langspec, SemanticOptions::default())
}

/// Performs semantic analysis on one parsed script with explicit options.
pub fn analyze_script_with_options(
    script: &Script,
    langspec: Option<&LangSpec>,
    options: SemanticOptions,
) -> Result<SemanticModel, SemanticError> {
    Analyzer::new(script, langspec, options).analyze()
}

#[derive(Debug, Clone, PartialEq)]
enum ConstantValue {
    Int(i32),
    Float(f32),
    String(String),
    ObjectId(i32),
    ObjectSelf,
    ObjectInvalid,
    LocationInvalid,
    Json(String),
    Vector([f32; 3]),
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
}

#[derive(Debug, Default)]
struct SwitchContext {
    case_values:       BTreeSet<i32>,
    has_default:       bool,
    scope_decl_counts: Vec<usize>,
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

struct Analyzer<'a> {
    script:            &'a Script,
    options:           SemanticOptions,
    builtin_constants: BTreeMap<String, ConstantValue>,
    global_constants:  BTreeMap<String, ConstantValue>,
    functions:         BTreeMap<String, FunctionInfo>,
    structs:           BTreeMap<String, SemanticStruct>,
    globals:           BTreeMap<String, SemanticGlobal>,
}

impl<'a> Analyzer<'a> {
    fn new(script: &'a Script, langspec: Option<&LangSpec>, options: SemanticOptions) -> Self {
        let mut builtin_constants = BTreeMap::new();
        let mut functions = BTreeMap::new();

        if let Some(langspec) = langspec {
            for constant in &langspec.constants {
                if let Some(value) = constant_from_builtin_value(&constant.value) {
                    builtin_constants.insert(constant.name.clone(), value);
                }
            }

            for function in &langspec.functions {
                let parameters = function
                    .parameters
                    .iter()
                    .map(|parameter| SemanticParameter {
                        name:        parameter.name.clone(),
                        ty:          semantic_type_from_builtin_type(&parameter.ty),
                        is_optional: parameter.default.is_some(),
                        default:     parameter
                            .default
                            .as_ref()
                            .and_then(literal_from_builtin_value),
                    })
                    .collect::<Vec<_>>();

                functions.insert(
                    function.name.clone(),
                    FunctionInfo {
                        signature:        SemanticFunction {
                            name: function.name.clone(),
                            return_type: semantic_type_from_builtin_type(&function.return_type),
                            parameters,
                            has_body: false,
                            is_builtin: true,
                        },
                        declaration_span: crate::Span::new(crate::SourceId::new(0), 0, 0),
                    },
                );
            }
        }

        Self {
            script,
            options,
            builtin_constants,
            global_constants: BTreeMap::new(),
            functions,
            structs: BTreeMap::new(),
            globals: BTreeMap::new(),
        }
    }

    fn analyze(mut self) -> Result<SemanticModel, SemanticError> {
        self.collect_structs()?;
        self.collect_const_globals_for_function_defaults()?;
        self.collect_functions()?;
        self.collect_globals()?;
        self.analyze_function_bodies()?;
        self.validate_entrypoint()?;

        Ok(SemanticModel {
            structs:   self.structs,
            globals:   self.globals,
            functions: self
                .functions
                .into_iter()
                .map(|(name, info)| (name, info.signature))
                .collect(),
        })
    }

    fn collect_const_globals_for_function_defaults(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Global(declaration) = item else {
                continue;
            };
            if !declaration.ty.is_const {
                continue;
            }

            let ty = self.resolve_type(&declaration.ty)?;
            for declarator in &declaration.declarators {
                let value = if let Some(initializer) = &declarator.initializer {
                    self.evaluate_constant_expr(initializer).ok_or_else(|| {
                        SemanticError::new(
                            CompilerErrorCode::InvalidValueAssignedToConstant,
                            initializer.span,
                            format!(
                                "constant {:?} must be initialized with a constant value",
                                declarator.name
                            ),
                        )
                    })?
                } else {
                    default_constant_value(&ty).ok_or_else(|| {
                        SemanticError::new(
                            CompilerErrorCode::InvalidValueAssignedToConstant,
                            declarator.span,
                            format!(
                                "constant {:?} type {:?} does not support default constant \
                                 initialization",
                                declarator.name, ty
                            ),
                        )
                    })?
                };
                if !types_compatible(&ty, &value.ty()) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::InvalidValueAssignedToConstant,
                        declarator
                            .initializer
                            .as_ref()
                            .map_or(declarator.span, |initializer| initializer.span),
                        format!(
                            "constant {:?} initializer does not match type {:?}",
                            declarator.name, ty
                        ),
                    ));
                }

                self.global_constants.insert(declarator.name.clone(), value);
            }
        }

        Ok(())
    }

    fn collect_structs(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Struct(definition) = item else {
                continue;
            };

            if self.structs.contains_key(&definition.name) {
                return Err(SemanticError::new(
                    CompilerErrorCode::StructureRedefined,
                    definition.span,
                    format!("structure {:?} was defined more than once", definition.name),
                ));
            }

            let mut fields = Vec::new();
            let mut seen_names = BTreeMap::<String, crate::Span>::new();
            for declaration in &definition.fields {
                let field_type = self.resolve_type(&declaration.ty)?;
                for field in &declaration.names {
                    if seen_names.contains_key(&field.name) {
                        return Err(SemanticError::new(
                            CompilerErrorCode::VariableUsedTwiceInSameStructure,
                            field.span,
                            format!(
                                "field {:?} was used twice in structure {:?}",
                                field.name, definition.name
                            ),
                        ));
                    }
                    seen_names.insert(field.name.clone(), field.span);
                    fields.push(SemanticField {
                        name: field.name.clone(),
                        ty:   field_type.clone(),
                    });
                }
            }

            self.structs.insert(
                definition.name.clone(),
                SemanticStruct {
                    name: definition.name.clone(),
                    fields,
                },
            );
        }

        Ok(())
    }

    fn collect_functions(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Function(function) = item else {
                continue;
            };

            let signature = self.resolve_function_signature(function)?;
            if let Some(existing) = self.functions.get_mut(&function.name) {
                if existing.signature.is_builtin {
                    return Err(SemanticError::new(
                        CompilerErrorCode::FunctionImplementationAndDefinitionDiffer,
                        function.span,
                        format!(
                            "function {:?} conflicts with builtin declaration",
                            function.name
                        ),
                    ));
                }

                if existing.signature.return_type != signature.return_type
                    || !parameters_match(&existing.signature.parameters, &signature.parameters)
                {
                    return Err(SemanticError::new(
                        CompilerErrorCode::FunctionImplementationAndDefinitionDiffer,
                        function.span,
                        format!(
                            "function {:?} declaration differs from previous declaration",
                            function.name
                        ),
                    ));
                }

                for (existing_parameter, new_parameter) in existing
                    .signature
                    .parameters
                    .iter_mut()
                    .zip(&signature.parameters)
                {
                    if existing_parameter.default.is_none() && new_parameter.default.is_some() {
                        existing_parameter.default = new_parameter.default.clone();
                        existing_parameter.is_optional = true;
                    }
                }

                if function.body.is_some() {
                    if existing.signature.has_body {
                        return Err(SemanticError::new(
                            CompilerErrorCode::DuplicateFunctionImplementation,
                            function.span,
                            format!(
                                "function {:?} was implemented more than once",
                                function.name
                            ),
                        ));
                    }
                    existing.signature.has_body = true;
                }
                continue;
            }

            self.functions.insert(
                function.name.clone(),
                FunctionInfo {
                    declaration_span: function.span,
                    signature,
                },
            );
        }

        Ok(())
    }

    fn collect_globals(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Global(declaration) = item else {
                continue;
            };

            let ty = self.resolve_type(&declaration.ty)?;
            for declarator in &declaration.declarators {
                if self.globals.contains_key(&declarator.name) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::VariableAlreadyUsedWithinScope,
                        declarator.span,
                        format!("global {:?} was declared more than once", declarator.name),
                    ));
                }

                if declaration.ty.is_const {
                    let value = if let Some(initializer) = &declarator.initializer {
                        self.evaluate_constant_expr(initializer).ok_or_else(|| {
                            SemanticError::new(
                                CompilerErrorCode::InvalidValueAssignedToConstant,
                                initializer.span,
                                format!(
                                    "constant {:?} must be initialized with a constant value",
                                    declarator.name
                                ),
                            )
                        })?
                    } else {
                        default_constant_value(&ty).ok_or_else(|| {
                            SemanticError::new(
                                CompilerErrorCode::InvalidValueAssignedToConstant,
                                declarator.span,
                                format!(
                                    "constant {:?} type {:?} does not support default constant \
                                     initialization",
                                    declarator.name, ty
                                ),
                            )
                        })?
                    };
                    if !types_compatible(&ty, &value.ty()) {
                        return Err(SemanticError::new(
                            CompilerErrorCode::InvalidValueAssignedToConstant,
                            declarator
                                .initializer
                                .as_ref()
                                .map_or(declarator.span, |initializer| initializer.span),
                            format!(
                                "constant {:?} initializer does not match type {:?}",
                                declarator.name, ty
                            ),
                        ));
                    }

                    self.global_constants
                        .insert(declarator.name.clone(), value.clone());
                } else if let Some(initializer) = &declarator.initializer {
                    let initializer_type = self
                        .analyze_expr(initializer, &mut Vec::new())
                        .map(|resolved| resolved.ty)?;
                    if !types_compatible(&ty, &initializer_type) {
                        return Err(SemanticError::new(
                            CompilerErrorCode::MismatchedTypes,
                            initializer.span,
                            format!(
                                "initializer for global {:?} has type {:?}, expected {:?}",
                                declarator.name, initializer_type, ty
                            ),
                        ));
                    }
                }

                self.globals.insert(
                    declarator.name.clone(),
                    SemanticGlobal {
                        name:     declarator.name.clone(),
                        ty:       ty.clone(),
                        is_const: declaration.ty.is_const,
                    },
                );
            }
        }

        Ok(())
    }

    fn analyze_function_bodies(&self) -> Result<(), SemanticError> {
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
                &info.signature.return_type,
                true,
                &mut context,
            )?;

            if info.signature.return_type != SemanticType::Void
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

    fn analyze_block(
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
            self.analyze_stmt(statement, scopes, return_type, context)?;
        }

        if !is_function_body {
            context.exit_scope();
            scopes.pop();
        }

        Ok(())
    }

    fn analyze_stmt(
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

                    if let Some(initializer) = &declarator.initializer {
                        let initializer_type = self.analyze_expr(initializer, scopes)?.ty;
                        if !types_compatible(&ty, &initializer_type) {
                            return Err(SemanticError::new(
                                CompilerErrorCode::MismatchedTypes,
                                initializer.span,
                                format!(
                                    "initializer for {:?} has type {:?}, expected {:?}",
                                    declarator.name, initializer_type, ty
                                ),
                            ));
                        }
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
                }
                context.record_declaration();
                Ok(())
            }
            Stmt::Expression(statement) => {
                self.analyze_expr(&statement.expr, scopes)?;
                Ok(())
            }
            Stmt::If(statement) => {
                let condition = self.analyze_expr(&statement.condition, scopes)?;
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
                let condition = self.analyze_expr(&statement.condition, scopes)?;
                if condition.ty != SemanticType::Int {
                    return Err(SemanticError::new(
                        CompilerErrorCode::SwitchMustEvaluateToAnInteger,
                        statement.condition.span,
                        "switch condition must evaluate to int",
                    ));
                }
                context.switch_stack.push(SwitchContext {
                    case_values:       BTreeSet::new(),
                    has_default:       false,
                    scope_decl_counts: vec![0],
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
                    let actual = self.analyze_expr(value, scopes)?.ty;
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
                let condition = self.analyze_expr(&statement.condition, scopes)?;
                if condition.ty != SemanticType::Int {
                    return Err(SemanticError::new(
                        CompilerErrorCode::NonIntegerExpressionWhereIntegerRequired,
                        statement.condition.span,
                        "while condition must evaluate to int",
                    ));
                }
                self.analyze_stmt(&statement.body, scopes, return_type, context)
            }
            Stmt::DoWhile(statement) => {
                self.analyze_stmt(&statement.body, scopes, return_type, context)?;
                let condition = self.analyze_expr(&statement.condition, scopes)?;
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
                    self.analyze_expr(initializer, scopes)?;
                }
                if let Some(condition) = &statement.condition {
                    let resolved = self.analyze_expr(condition, scopes)?;
                    if resolved.ty != SemanticType::Int {
                        return Err(SemanticError::new(
                            CompilerErrorCode::NonIntegerExpressionWhereIntegerRequired,
                            condition.span,
                            "for condition must evaluate to int",
                        ));
                    }
                }
                if let Some(update) = &statement.update {
                    self.analyze_expr(update, scopes)?;
                }
                self.analyze_stmt(&statement.body, scopes, return_type, context)
            }
            Stmt::Case(statement) => {
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
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty(_) => Ok(()),
        }
    }

    fn analyze_expr(
        &self,
        expr: &Expr,
        scopes: &mut Vec<BTreeMap<String, ScopeBinding>>,
    ) -> Result<ResolvedExpr, SemanticError> {
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
                    let resolved = self.analyze_expr(argument, scopes)?;
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
                        return Err(SemanticError::new(
                            CompilerErrorCode::DeclarationDoesNotMatchParameters,
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
                let resolved_base = self.analyze_expr(base, scopes)?;
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
                let resolved = self.analyze_expr(inner, scopes)?;
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
                let left = self.analyze_expr(left, scopes)?;
                let right = self.analyze_expr(right, scopes)?;
                let ty = Self::binary_result_type(*op, &left.ty, &right.ty, expr.span)?;
                Ok(ResolvedExpr {
                    ty,
                    is_lvalue: false,
                    is_const: left.is_const && right.is_const,
                })
            }
            ExprKind::Conditional {
                condition: _,
                when_true,
                when_false,
            } => {
                let when_true = self.analyze_expr(when_true, scopes)?;
                let when_false = self.analyze_expr(when_false, scopes)?;
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
                let left_resolved = self.analyze_expr(left, scopes)?;
                if !left_resolved.is_lvalue {
                    return Err(SemanticError::new(
                        CompilerErrorCode::BadLValue,
                        left.span,
                        "left side of assignment must be an lvalue",
                    ));
                }
                let right_resolved = self.analyze_expr(right, scopes)?;
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
                        CompilerErrorCode::MismatchedTypes,
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

    fn binary_result_type(
        op: BinaryOp,
        left: &SemanticType,
        right: &SemanticType,
        span: crate::Span,
    ) -> Result<SemanticType, SemanticError> {
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

    fn resolve_function_signature(
        &self,
        function: &FunctionDecl,
    ) -> Result<SemanticFunction, SemanticError> {
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
                default:     default.as_ref().and_then(literal_from_constant_value),
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

    fn resolve_type(&self, ty: &TypeSpec) -> Result<SemanticType, SemanticError> {
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
        }
    }

    fn evaluate_constant_expr(&self, expr: &Expr) -> Option<ConstantValue> {
        match &expr.kind {
            ExprKind::Literal(literal) => constant_from_literal(literal),
            ExprKind::Identifier(name) => self.lookup_constant(name),
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
            ExprKind::Unary {
                op:
                    UnaryOp::PreIncrement
                    | UnaryOp::PreDecrement
                    | UnaryOp::PostIncrement
                    | UnaryOp::PostDecrement,
                ..
            } => None,
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
            _ => None,
        }
    }

    fn evaluate_function_default_expr(&self, expr: &Expr) -> Option<ConstantValue> {
        self.evaluate_constant_expr(expr)
    }

    fn evaluate_constant_binary(
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
            _ => None,
        }
    }

    fn evaluate_switch_case_value(&self, expr: &Expr) -> Option<i32> {
        match self.evaluate_constant_expr(expr)? {
            ConstantValue::Int(value) => Some(value),
            ConstantValue::String(value) => Some(nwscript_string_hash(&value)),
            _ => None,
        }
    }

    fn lookup_constant(&self, name: &str) -> Option<ConstantValue> {
        self.global_constants
            .get(name)
            .cloned()
            .or_else(|| self.builtin_constants.get(name).cloned())
    }

    fn lookup_value(
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

    fn validate_entrypoint(&self) -> Result<(), SemanticError> {
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

        if self.options.allow_conditional_script {
            if let Some(function) = self.functions.get("StartingConditional") {
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
            return Err(SemanticError::new(
                CompilerErrorCode::NoFunctionIntscInScript,
                crate::Span::new(crate::SourceId::new(0), 0, 0),
                "script must define StartingConditional",
            ));
        }

        Err(SemanticError::new(
            CompilerErrorCode::NoFunctionMainInScript,
            crate::Span::new(crate::SourceId::new(0), 0, 0),
            "script must define main",
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedExpr {
    ty:        SemanticType,
    is_lvalue: bool,
    is_const:  bool,
}

fn semantic_type_from_builtin_type(ty: &BuiltinType) -> SemanticType {
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

fn constant_from_builtin_value(value: &BuiltinValue) -> Option<ConstantValue> {
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

fn literal_from_builtin_value(value: &BuiltinValue) -> Option<Literal> {
    constant_from_builtin_value(value).and_then(|value| literal_from_constant_value(&value))
}

fn constant_from_literal(literal: &Literal) -> Option<ConstantValue> {
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

fn literal_from_constant_value(value: &ConstantValue) -> Option<Literal> {
    match value {
        ConstantValue::Int(value) => Some(Literal::Integer(*value)),
        ConstantValue::Float(value) => Some(Literal::Float(*value)),
        ConstantValue::String(value) => Some(Literal::String(value.clone())),
        ConstantValue::ObjectId(value) => Some(Literal::Integer(*value)),
        ConstantValue::ObjectSelf => Some(Literal::ObjectSelf),
        ConstantValue::ObjectInvalid => Some(Literal::ObjectInvalid),
        ConstantValue::LocationInvalid => Some(Literal::LocationInvalid),
        ConstantValue::Json(value) => Some(Literal::Json(value.clone())),
        ConstantValue::Vector(value) => Some(Literal::Vector(*value)),
    }
}

fn semantic_type_from_literal(literal: &Literal) -> SemanticType {
    match literal {
        Literal::Integer(_) => SemanticType::Int,
        Literal::Float(_) => SemanticType::Float,
        Literal::String(_) => SemanticType::String,
        Literal::ObjectSelf | Literal::ObjectInvalid => SemanticType::Object,
        Literal::LocationInvalid => SemanticType::EngineStructure("location".to_string()),
        Literal::Json(_) => SemanticType::EngineStructure("json".to_string()),
        Literal::Vector(_) => SemanticType::Vector,
        Literal::Magic(MagicLiteral::Line) => SemanticType::Int,
        Literal::Magic(
            MagicLiteral::Function | MagicLiteral::File | MagicLiteral::Date | MagicLiteral::Time,
        ) => SemanticType::String,
    }
}

fn default_constant_value(ty: &SemanticType) -> Option<ConstantValue> {
    match ty {
        SemanticType::Int => Some(ConstantValue::Int(0)),
        SemanticType::Float => Some(ConstantValue::Float(0.0)),
        SemanticType::String => Some(ConstantValue::String(String::new())),
        _ => None,
    }
}

fn evaluate_int_constant_binary(op: BinaryOp, left: i32, right: i32) -> Option<i32> {
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
        BinaryOp::ShiftLeft => Some(left.wrapping_shl(right as u32)),
        BinaryOp::ShiftRight => Some(left.wrapping_shr(right as u32)),
        BinaryOp::UnsignedShiftRight => Some(((left as u32).wrapping_shr(right as u32)) as i32),
        BinaryOp::Add => Some(left.wrapping_add(right)),
        BinaryOp::Subtract => Some(left.wrapping_sub(right)),
        BinaryOp::Multiply => Some(left.wrapping_mul(right)),
        BinaryOp::Divide => left.checked_div(right),
        BinaryOp::Modulus => left.checked_rem(right),
    }
}

fn evaluate_float_constant_binary(op: BinaryOp, left: f32, right: f32) -> Option<ConstantValue> {
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

fn evaluate_string_constant_binary(op: BinaryOp, left: &str, right: &str) -> Option<ConstantValue> {
    match op {
        BinaryOp::Add => {
            if left.len().saturating_add(right.len()) >= 0x8000 {
                None
            } else {
                Some(ConstantValue::String(format!("{left}{right}")))
            }
        }
        BinaryOp::EqualEqual => Some(ConstantValue::Int(i32::from(left == right))),
        BinaryOp::NotEqual => Some(ConstantValue::Int(i32::from(left != right))),
        _ => None,
    }
}

fn type_supports_optional_parameter(ty: &SemanticType) -> bool {
    match ty {
        SemanticType::Int
        | SemanticType::Float
        | SemanticType::String
        | SemanticType::Object
        | SemanticType::Vector => true,
        SemanticType::EngineStructure(name) => name == "location" || name == "json",
        _ => false,
    }
}

fn types_compatible(expected: &SemanticType, actual: &SemanticType) -> bool {
    expected == actual
}

fn parameters_match(left: &[SemanticParameter], right: &[SemanticParameter]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.ty == right.ty)
}

fn insert_scope_binding(
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

fn current_scope_contains(scopes: &[BTreeMap<String, ScopeBinding>], name: &str) -> bool {
    scopes.last().is_some_and(|scope| scope.contains_key(name))
}

fn statement_guarantees_return(statement: &Stmt) -> bool {
    match statement {
        Stmt::Return(_) => true,
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

#[cfg(test)]
mod tests {
    use super::{SemanticType, analyze_script, analyze_script_with_options};
    use crate::{LangSpec, Literal, SemanticOptions, SourceId, parse_text};

    fn test_langspec() -> LangSpec {
        LangSpec {
            engine_num_structures: 3,
            engine_structures:     vec![
                "effect".to_string(),
                "location".to_string(),
                "json".to_string(),
            ],
            constants:             vec![
                crate::BuiltinConstant {
                    name:  "TRUE".to_string(),
                    ty:    crate::BuiltinType::Int,
                    value: crate::BuiltinValue::Int(1),
                },
                crate::BuiltinConstant {
                    name:  "FALSE".to_string(),
                    ty:    crate::BuiltinType::Int,
                    value: crate::BuiltinValue::Int(0),
                },
                crate::BuiltinConstant {
                    name:  "OBJECT_INVALID".to_string(),
                    ty:    crate::BuiltinType::Object,
                    value: crate::BuiltinValue::ObjectInvalid,
                },
            ],
            functions:             vec![
                crate::BuiltinFunction {
                    name:        "DelayCommand".to_string(),
                    return_type: crate::BuiltinType::Void,
                    parameters:  vec![
                        crate::BuiltinParameter {
                            name:    "fSeconds".to_string(),
                            ty:      crate::BuiltinType::Float,
                            default: None,
                        },
                        crate::BuiltinParameter {
                            name:    "aAction".to_string(),
                            ty:      crate::BuiltinType::Action,
                            default: None,
                        },
                    ],
                },
                crate::BuiltinFunction {
                    name:        "EffectDamage".to_string(),
                    return_type: crate::BuiltinType::EngineStructure("effect".to_string()),
                    parameters:  vec![crate::BuiltinParameter {
                        name:    "nAmount".to_string(),
                        ty:      crate::BuiltinType::Int,
                        default: None,
                    }],
                },
            ],
        }
    }

    #[test]
    fn resolves_functions_globals_and_structs() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(40),
            "struct Foo { int value; }; effect gFx; void helper(int n = TRUE); void helper(int n \
             = TRUE) { int x = n; } void main() { struct Foo f; int x = f.value; }",
            Some(&test_langspec()),
        )?;

        let model = analyze_script(&script, Some(&test_langspec()))?;
        assert!(model.structs.contains_key("Foo"));
        assert!(model.functions.contains_key("helper"));
        assert!(model.functions.contains_key("EffectDamage"));
        assert_eq!(
            model
                .globals
                .get("gFx")
                .ok_or_else(|| std::io::Error::other("missing global gFx"))?
                .ty,
            SemanticType::EngineStructure("effect".to_string())
        );
        Ok(())
    }

    #[test]
    fn rejects_optional_parameter_ordering() {
        let script = parse_text(
            SourceId::new(41),
            "void helper(int n = 1, int m); void main() { return; }",
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let error =
            analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            error.code,
            crate::CompilerErrorCode::NonOptionalParameterCannotFollowOptionalParameter
        );
    }

    #[test]
    fn folds_constant_globals_and_uses_default_constant_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(46),
            r#"
                const int MASK = (1 + 2) * 4;
                const int ZERO;
                const string LABEL = "ab" + "cd";
                const int PICK = TRUE ? 7 : 9;
                void main() {
                    int x = MASK;
                    int y = ZERO;
                    string s = LABEL;
                    int z = PICK;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let model = analyze_script(&script, Some(&test_langspec()))?;
        assert_eq!(
            model.globals.get("MASK").map(|global| global.is_const),
            Some(true)
        );
        assert_eq!(
            model.globals.get("ZERO").map(|global| global.is_const),
            Some(true)
        );
        assert_eq!(
            model.globals.get("LABEL").map(|global| global.is_const),
            Some(true)
        );
        assert_eq!(
            model.globals.get("PICK").map(|global| global.is_const),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn accepts_case_labels_backed_by_const_globals() {
        let script = parse_text(
            SourceId::new(47),
            r#"
                const int CASE_A = 1 + 2;
                const int CASE_B = FALSE ? 8 : 4;
                void main() {
                    int n = 0;
                    switch (n) {
                        case CASE_A:
                            break;
                        case CASE_B:
                            break;
                        default:
                            break;
                    }
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");

        analyze_script(&script, Some(&test_langspec()))
            .expect("const global should be valid in case label");
    }

    #[test]
    fn accepts_function_defaults_backed_by_const_globals_and_expressions() {
        let script = parse_text(
            SourceId::new(48),
            r#"
                const int EXECUTE_END = 1;
                const int POLICY_DEFAULT = EXECUTE_END + 2;
                void helper(int nValue = EXECUTE_END, int nPolicy = POLICY_DEFAULT) {
                    return;
                }
                void main() {
                    helper();
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");

        analyze_script(&script, Some(&test_langspec()))
            .expect("const global defaults should resolve in function signatures");
    }

    #[test]
    fn accepts_string_case_labels_by_upstream_hash_rule() {
        let script = parse_text(
            SourceId::new(49),
            r#"
                const string LABEL = "abc";
                void main() {
                    int n = 0;
                    switch (n) {
                        case "abc":
                            break;
                        case LABEL:
                            break;
                    }
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let error =
            analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            error.code,
            crate::CompilerErrorCode::MultipleCaseConstantStatementsWithinSwitch
        );
    }

    #[test]
    fn rejects_duplicate_default_labels_within_one_switch() {
        let script = parse_text(
            SourceId::new(50),
            r#"
                void main() {
                    int n = 0;
                    switch (n) {
                        default:
                            break;
                        default:
                            break;
                    }
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let error =
            analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            error.code,
            crate::CompilerErrorCode::MultipleDefaultStatementsWithinSwitch
        );
    }

    #[test]
    fn rejects_case_and_default_labels_that_jump_over_live_declarations() {
        let case_script = parse_text(
            SourceId::new(51),
            r#"
                void main() {
                    int n = 0;
                    switch (n) {
                        int x = 1;
                        case 1:
                            break;
                    }
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let case_error =
            analyze_script(&case_script, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            case_error.code,
            crate::CompilerErrorCode::JumpingOverDeclarationStatementsCaseDisallowed
        );

        let default_script = parse_text(
            SourceId::new(52),
            r#"
                void main() {
                    int n = 0;
                    switch (n) {
                        int x = 1;
                        default:
                            break;
                    }
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let default_error = analyze_script(&default_script, Some(&test_langspec()))
            .expect_err("analysis should fail");
        assert_eq!(
            default_error.code,
            crate::CompilerErrorCode::JumpingOverDeclarationStatementsDefaultDisallowed
        );
    }

    #[test]
    fn duplicate_case_detection_is_scoped_to_the_innermost_switch() {
        let script = parse_text(
            SourceId::new(53),
            r#"
                void main() {
                    int n = 0;
                    switch (n) {
                        case 1:
                            switch (n) {
                                case 1:
                                    break;
                                default:
                                    break;
                            }
                            break;
                        default:
                            break;
                    }
                }
            "#,
            Some(&test_langspec()),
        )
        .expect("script should parse");

        analyze_script(&script, Some(&test_langspec()))
            .expect("nested switch labels should be tracked independently");
    }

    #[test]
    fn parser_accepts_constant_function_default_expressions_like_upstream() {
        let script = parse_text(
            SourceId::new(48),
            "void helper(int n = 1 + 2); void main() {}",
            Some(&test_langspec()),
        )
        .expect("parser should accept constant default expressions");

        analyze_script(&script, Some(&test_langspec()))
            .expect("semantic analysis should accept constant default expressions");
    }

    #[test]
    fn rejects_undefined_identifiers_and_bad_field_access() {
        let script = parse_text(
            SourceId::new(42),
            "struct Foo { int value; }; void main() { struct Foo f; int x = f.missing; }",
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let error =
            analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
        assert!(matches!(
            error.code,
            crate::CompilerErrorCode::UndefinedFieldInStructure
                | crate::CompilerErrorCode::UndefinedIdentifier
        ));
    }

    #[test]
    fn action_parameters_require_direct_void_calls() {
        let valid = parse_text(
            SourceId::new(54),
            "void helper() {} void main() { DelayCommand(1.0, helper()); }",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        analyze_script(&valid, Some(&test_langspec()))
            .expect("void call should be valid for action parameter");

        let invalid = parse_text(
            SourceId::new(55),
            "void main() { DelayCommand(1.0, EffectDamage(1)); }",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let error =
            analyze_script(&invalid, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            error.code,
            crate::CompilerErrorCode::DeclarationDoesNotMatchParameters
        );
    }

    #[test]
    fn function_name_reuse_requires_identical_parameter_lists() {
        let mismatch = parse_text(
            SourceId::new(56),
            "void helper(int n); void helper(float n); void main() {}",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let mismatch_error =
            analyze_script(&mismatch, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            mismatch_error.code,
            crate::CompilerErrorCode::FunctionImplementationAndDefinitionDiffer
        );

        let return_mismatch = parse_text(
            SourceId::new(65),
            "int helper(int n); void helper(int n) {} void main() {}",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let return_mismatch_error = analyze_script(&return_mismatch, Some(&test_langspec()))
            .expect_err("analysis should fail");
        assert_eq!(
            return_mismatch_error.code,
            crate::CompilerErrorCode::FunctionImplementationAndDefinitionDiffer
        );

        let duplicate_impl = parse_text(
            SourceId::new(57),
            "void helper(int n) {} void helper(int n) {} void main() {}",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let duplicate_impl_error = analyze_script(&duplicate_impl, Some(&test_langspec()))
            .expect_err("analysis should fail");
        assert_eq!(
            duplicate_impl_error.code,
            crate::CompilerErrorCode::DuplicateFunctionImplementation
        );
    }

    #[test]
    fn function_redeclarations_may_add_or_remove_trailing_defaults_like_upstream() {
        let later_default = parse_text(
            SourceId::new(60),
            "void helper(int n); void helper(int n = 1) {} void main() {}",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        analyze_script(&later_default, Some(&test_langspec()))
            .expect("later implementation default should be accepted");

        let earlier_default = parse_text(
            SourceId::new(61),
            "void helper(int n = 1); void helper(int n) {} void main() {}",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let earlier_default_semantic = analyze_script(&earlier_default, Some(&test_langspec()))
            .expect("later implementation without default should be accepted");
        let helper = earlier_default_semantic
            .functions
            .get("helper")
            .expect("helper should be present in the semantic model");
        let first_parameter = helper
            .parameters
            .first()
            .expect("helper should have one parameter");
        assert_eq!(
            first_parameter.default,
            Some(Literal::Integer(1)),
            "forward declaration defaults should survive into the merged signature"
        );

        let renamed_parameter = parse_text(
            SourceId::new(64),
            "int helper(int nDurationType); int helper(int nDurationCompare) { return \
             nDurationCompare; } void main() {}",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        analyze_script(&renamed_parameter, Some(&test_langspec()))
            .expect("implementation parameter names should be visible inside the body");
    }

    #[test]
    fn nested_scopes_may_shadow_outer_names_but_same_scope_duplicates_fail() {
        let shadowing = parse_text(
            SourceId::new(62),
            "int g = 1; void main() { int x = g; { int x = 2; int y = x; } int z = x; }",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        analyze_script(&shadowing, Some(&test_langspec()))
            .expect("nested scopes should be allowed to shadow outer names");

        let duplicate = parse_text(
            SourceId::new(63),
            "void main() { int x = 1; int x = 2; }",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        let duplicate_error =
            analyze_script(&duplicate, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            duplicate_error.code,
            crate::CompilerErrorCode::VariableAlreadyUsedWithinScope
        );
    }

    #[test]
    fn function_body_scope_may_shadow_parameter_names() {
        let script = parse_text(
            SourceId::new(65),
            "int helper(object oSpellTarget) { object oSpellTarget = OBJECT_SELF; return TRUE; } \
             void main() {}",
            Some(&test_langspec()),
        )
        .expect("script should parse");
        analyze_script(&script, Some(&test_langspec()))
            .expect("function body locals should be allowed to shadow parameter names");
    }

    #[test]
    fn rejects_local_const_declarations_like_upstream() {
        let script = parse_text(
            SourceId::new(43),
            "void main() { const int x = 1; }",
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let error =
            analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            error.code,
            crate::CompilerErrorCode::ConstKeywordCannotBeUsedOnNonGlobalVariables
        );
    }

    #[test]
    fn rejects_missing_return_paths() {
        let script = parse_text(
            SourceId::new(44),
            "int main() { if (TRUE) { return 1; } }",
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let error =
            analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
        assert_eq!(
            error.code,
            crate::CompilerErrorCode::NotAllControlPathsReturnAValue
        );
    }

    #[test]
    fn validates_required_entrypoints_when_requested() {
        let script = parse_text(
            SourceId::new(45),
            "int helper() { return 1; }",
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let error = analyze_script_with_options(
            &script,
            Some(&test_langspec()),
            SemanticOptions {
                require_entrypoint:       true,
                allow_conditional_script: false,
            },
        )
        .expect_err("analysis should fail");
        assert_eq!(error.code, crate::CompilerErrorCode::NoFunctionMainInScript);
    }
}
