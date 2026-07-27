use std::{collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{CompilerErrorCode, EnumBackingType, Literal};

/// Maximum number of parameters accepted by the native compiler.
pub const MAX_FUNCTION_PARAMETERS: usize = 32;

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
    pub(super) fn new(
        code: CompilerErrorCode,
        span: crate::Span,
        message: impl Into<String>,
    ) -> Self {
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
    /// One strongly typed enum lowered to its native storage representation.
    Enum {
        /// Source enum name.
        name:    String,
        /// Native storage representation.
        backing: EnumBackingType,
    },
}

/// One resolved enum variant value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticEnumValue {
    /// Integer discriminant.
    Int(i32),
    /// Exact string discriminant.
    String(crate::ScriptString),
}

/// One resolved strongly typed enum variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticEnumVariant {
    /// Variant name.
    pub name:       String,
    /// Resolved value.
    pub value:      SemanticEnumValue,
    /// Whether this is the enum's selected default.
    pub is_default: bool,
    /// Global compatibility aliases.
    pub aliases:    Vec<String>,
}

/// One resolved strongly typed enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticEnum {
    /// Enum type name.
    pub name:            String,
    /// Native storage representation.
    pub backing:         EnumBackingType,
    /// Variants in declaration order.
    pub variants:        Vec<SemanticEnumVariant>,
    /// Index of the selected default variant.
    pub default_variant: usize,
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
    /// Resolved enums indexed by name.
    pub enums:     BTreeMap<String, SemanticEnum>,
    /// Fully resolved transparent aliases indexed by name.
    pub aliases:   BTreeMap<String, SemanticType>,
    /// Resolved structures indexed by name.
    pub structs:   BTreeMap<String, SemanticStruct>,
    /// Resolved global variables indexed by name.
    pub globals:   BTreeMap<String, SemanticGlobal>,
    /// Resolved functions indexed by name.
    pub functions: BTreeMap<String, SemanticFunction>,
}
