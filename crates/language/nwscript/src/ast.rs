use serde::{Deserialize, Serialize};

use crate::source::Span;

/// One parsed `NWScript` translation unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Script {
    /// Top-level items in source order.
    pub items: Vec<TopLevelItem>,
}

/// One top-level `NWScript` item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TopLevelItem {
    /// One `#include "..."` directive.
    Include(IncludeDirective),
    /// One global variable declaration statement.
    Global(Declaration),
    /// One function declaration or definition.
    Function(FunctionDecl),
    /// One user-defined `struct`.
    Struct(StructDecl),
}

/// One `#include "..."` directive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncludeDirective {
    /// Source span covering the directive.
    pub span: Span,
    /// Included script path payload.
    pub path: String,
}

/// One parsed function declaration or definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDecl {
    /// Source span covering the whole declaration or definition.
    pub span:        Span,
    /// Function return type.
    pub return_type: TypeSpec,
    /// Function name.
    pub name:        String,
    /// Function parameters in source order.
    pub parameters:  Vec<Parameter>,
    /// Optional function body. `None` means this was only a declaration.
    pub body:        Option<BlockStmt>,
}

/// One user-defined structure declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDecl {
    /// Source span covering the whole declaration.
    pub span:   Span,
    /// Structure name.
    pub name:   String,
    /// Field declarations in source order.
    pub fields: Vec<StructFieldDecl>,
}

/// One structure field declaration statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructFieldDecl {
    /// Source span covering the declaration.
    pub span:  Span,
    /// Field type.
    pub ty:    TypeSpec,
    /// Field names declared by this statement.
    pub names: Vec<NamedItem>,
}

/// One variable or field name plus span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedItem {
    /// Source span covering the identifier.
    pub span: Span,
    /// Identifier text.
    pub name: String,
}

/// One parsed parameter declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Parameter {
    /// Source span covering the declaration.
    pub span:    Span,
    /// Parameter type.
    pub ty:      TypeSpec,
    /// Parameter name.
    pub name:    String,
    /// Optional default value expression.
    pub default: Option<Expr>,
}

/// One parsed type specifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeSpec {
    /// Source span covering the type specifier.
    pub span:     Span,
    /// Whether `const` was present.
    pub is_const: bool,
    /// Underlying type shape.
    pub kind:     TypeKind,
}

/// One `NWScript` type kind recognized syntactically by the parser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
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
    /// `vector`
    Vector,
    /// `struct name`
    Struct(String),
    /// One builtin engine structure such as `effect` or `json`.
    EngineStructure(String),
}

/// One variable declaration statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Declaration {
    /// Source span covering the whole declaration.
    pub span:        Span,
    /// Declared type.
    pub ty:          TypeSpec,
    /// Declared variables.
    pub declarators: Vec<VarDeclarator>,
}

/// One declared variable, optionally with an initializer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VarDeclarator {
    /// Source span covering the declarator.
    pub span:        Span,
    /// Variable name.
    pub name:        String,
    /// Optional initializer expression.
    pub initializer: Option<Expr>,
}

/// One compound block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockStmt {
    /// Source span covering the block braces and contents.
    pub span:       Span,
    /// Statements inside the block.
    pub statements: Vec<Stmt>,
}

/// One statement in `NWScript` source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// `{ ... }`
    Block(BlockStmt),
    /// One declaration statement.
    Declaration(Declaration),
    /// One expression statement.
    Expression(ExpressionStmt),
    /// `if (...) ... [else ...]`
    If(IfStmt),
    /// `switch (...) ...`
    Switch(SwitchStmt),
    /// `return [expr];`
    Return(ReturnStmt),
    /// `while (...) ...`
    While(WhileStmt),
    /// `do ... while (...);`
    DoWhile(DoWhileStmt),
    /// `for (...; ...; ...) ...`
    For(ForStmt),
    /// `case expr:`
    Case(CaseStmt),
    /// `default:`
    Default(DefaultStmt),
    /// `break;`
    Break(SimpleStmt),
    /// `continue;`
    Continue(SimpleStmt),
    /// `;`
    Empty(SimpleStmt),
}

/// One statement carrying only a span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimpleStmt {
    /// Source span covering the statement.
    pub span: Span,
}

/// One expression statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpressionStmt {
    /// Source span covering the statement.
    pub span: Span,
    /// Parsed expression.
    pub expr: Expr,
}

/// One `if` statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IfStmt {
    /// Source span covering the whole statement.
    pub span:        Span,
    /// Condition expression.
    pub condition:   Expr,
    /// Statement executed when the condition is true.
    pub then_branch: Box<Stmt>,
    /// Optional `else` branch.
    pub else_branch: Option<Box<Stmt>>,
}

/// One `switch` statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SwitchStmt {
    /// Source span covering the whole statement.
    pub span:      Span,
    /// Condition expression.
    pub condition: Expr,
    /// Switch body.
    pub body:      Box<Stmt>,
}

/// One `return` statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnStmt {
    /// Source span covering the statement.
    pub span:  Span,
    /// Optional returned value.
    pub value: Option<Expr>,
}

/// One `while` statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WhileStmt {
    /// Source span covering the whole statement.
    pub span:      Span,
    /// Loop condition.
    pub condition: Expr,
    /// Loop body.
    pub body:      Box<Stmt>,
}

/// One `do ... while` statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoWhileStmt {
    /// Source span covering the whole statement.
    pub span:      Span,
    /// Loop body.
    pub body:      Box<Stmt>,
    /// Loop condition.
    pub condition: Expr,
}

/// One `for` statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForStmt {
    /// Source span covering the whole statement.
    pub span:        Span,
    /// Optional initializer expression.
    pub initializer: Option<Expr>,
    /// Optional loop condition.
    pub condition:   Option<Expr>,
    /// Optional update expression.
    pub update:      Option<Expr>,
    /// Loop body.
    pub body:        Box<Stmt>,
}

/// One `case` label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseStmt {
    /// Source span covering the label.
    pub span:  Span,
    /// Case condition expression.
    pub value: Expr,
}

/// One `default` label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DefaultStmt {
    /// Source span covering the label.
    pub span: Span,
}

/// One expression in `NWScript` source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expr {
    /// Source span covering the whole expression.
    pub span: Span,
    /// Expression shape.
    pub kind: ExprKind,
}

/// One expression shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExprKind {
    /// One literal constant.
    Literal(Literal),
    /// One variable or named constant reference.
    Identifier(String),
    /// One function call or action invocation.
    Call {
        /// Called expression.
        callee:    Box<Expr>,
        /// Call arguments in source order.
        arguments: Vec<Expr>,
    },
    /// One structure field access.
    FieldAccess {
        /// Expression on the left-hand side of `.`.
        base:  Box<Expr>,
        /// Field name on the right-hand side of `.`.
        field: String,
    },
    /// One unary or postfix operator.
    Unary {
        /// Applied operator.
        op:   UnaryOp,
        /// Operand expression.
        expr: Box<Expr>,
    },
    /// One binary operator.
    Binary {
        /// Applied operator.
        op:    BinaryOp,
        /// Left-hand operand.
        left:  Box<Expr>,
        /// Right-hand operand.
        right: Box<Expr>,
    },
    /// One conditional expression.
    Conditional {
        /// Condition before `?`.
        condition:  Box<Expr>,
        /// Expression between `?` and `:`.
        when_true:  Box<Expr>,
        /// Expression after `:`.
        when_false: Box<Expr>,
    },
    /// One assignment expression.
    Assignment {
        /// Applied assignment operator.
        op:    AssignmentOp,
        /// Assigned lvalue expression.
        left:  Box<Expr>,
        /// Right-hand expression.
        right: Box<Expr>,
    },
}

/// One unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    /// Prefix `-`
    Negate,
    /// Prefix `~`
    OnesComplement,
    /// Prefix `!`
    BooleanNot,
    /// Prefix `++`
    PreIncrement,
    /// Prefix `--`
    PreDecrement,
    /// Postfix `++`
    PostIncrement,
    /// Postfix `--`
    PostDecrement,
}

/// One binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    /// `*`
    Multiply,
    /// `/`
    Divide,
    /// `%`
    Modulus,
    /// `+`
    Add,
    /// `-`
    Subtract,
    /// `<<`
    ShiftLeft,
    /// `>>`
    ShiftRight,
    /// `>>>`
    UnsignedShiftRight,
    /// `>=`
    GreaterEqual,
    /// `>`
    GreaterThan,
    /// `<`
    LessThan,
    /// `<=`
    LessEqual,
    /// `!=`
    NotEqual,
    /// `==`
    EqualEqual,
    /// `&`
    BooleanAnd,
    /// `^`
    ExclusiveOr,
    /// `|`
    InclusiveOr,
    /// `&&`
    LogicalAnd,
    /// `||`
    LogicalOr,
}

/// One assignment operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignmentOp {
    /// `=`
    Assign,
    /// `-=`
    AssignMinus,
    /// `+=`
    AssignPlus,
    /// `*=`
    AssignMultiply,
    /// `/=`
    AssignDivide,
    /// `%=`
    AssignModulus,
    /// `&=`
    AssignAnd,
    /// `^=`
    AssignXor,
    /// `|=`
    AssignOr,
    /// `<<=`
    AssignShiftLeft,
    /// `>>=`
    AssignShiftRight,
    /// `>>>=`
    AssignUnsignedShiftRight,
}

/// One literal expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// One integer literal.
    Integer(i32),
    /// One floating-point literal.
    Float(f32),
    /// One string literal.
    String(String),
    /// `OBJECT_SELF`
    ObjectSelf,
    /// `OBJECT_INVALID`
    ObjectInvalid,
    /// `LOCATION_INVALID`
    LocationInvalid,
    /// One JSON constructor keyword lowered to its textual payload.
    Json(String),
    /// One vector constant.
    Vector([f32; 3]),
    /// One magic macro token.
    Magic(MagicLiteral),
}

/// One magic macro literal preserved in syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MagicLiteral {
    /// `__FUNCTION__`
    Function,
    /// `__FILE__`
    File,
    /// `__LINE__`
    Line,
    /// `__DATE__`
    Date,
    /// `__TIME__`
    Time,
}
