use serde::{Deserialize, Serialize};

use crate::source::Span;

/// Maximum token payload length used by the upstream compiler.
pub const MAX_TOKEN_LENGTH: usize = 65_536;

/// One `NWScript` keyword or builtin token recognized during lexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Keyword {
    /// `if`
    If,
    /// `do`
    Do,
    /// `else`
    Else,
    /// `int`
    Int,
    /// `float`
    Float,
    /// `string`
    String,
    /// `object`
    Object,
    /// `return`
    Return,
    /// `while`
    While,
    /// `for`
    For,
    /// `void`
    Void,
    /// `case`
    Case,
    /// `break`
    Break,
    /// `struct`
    Struct,
    /// `action`
    Action,
    /// `switch`
    Switch,
    /// `default`
    Default,
    /// `#include`
    Include,
    /// `continue`
    Continue,
    /// `vector`
    Vector,
    /// `const`
    Const,
    /// `#define`
    Define,
    /// `OBJECT_SELF`
    ObjectSelf,
    /// `OBJECT_INVALID`
    ObjectInvalid,
    /// `ENGINE_NUM_STRUCTURES`
    EngineNumStructuresDefinition,
    /// `ENGINE_STRUCTURE_0` through `ENGINE_STRUCTURE_9`
    EngineStructureDefinition,
    /// `JSON_NULL`
    JsonNull,
    /// `JSON_FALSE`
    JsonFalse,
    /// `JSON_TRUE`
    JsonTrue,
    /// `JSON_OBJECT`
    JsonObject,
    /// `JSON_ARRAY`
    JsonArray,
    /// `JSON_STRING`
    JsonString,
    /// `LOCATION_INVALID`
    LocationInvalid,
    /// `__FUNCTION__`
    FunctionMacro,
    /// `__FILE__`
    FileMacro,
    /// `__LINE__`
    LineMacro,
    /// `__DATE__`
    DateMacro,
    /// `__TIME__`
    TimeMacro,
}

impl Keyword {
    /// Returns the canonical source text for this keyword.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::If => "if",
            Self::Do => "do",
            Self::Else => "else",
            Self::Int => "int",
            Self::Float => "float",
            Self::String => "string",
            Self::Object => "object",
            Self::Return => "return",
            Self::While => "while",
            Self::For => "for",
            Self::Void => "void",
            Self::Case => "case",
            Self::Break => "break",
            Self::Struct => "struct",
            Self::Action => "action",
            Self::Switch => "switch",
            Self::Default => "default",
            Self::Include => "#include",
            Self::Continue => "continue",
            Self::Vector => "vector",
            Self::Const => "const",
            Self::Define => "#define",
            Self::ObjectSelf => "OBJECT_SELF",
            Self::ObjectInvalid => "OBJECT_INVALID",
            Self::EngineNumStructuresDefinition => "ENGINE_NUM_STRUCTURES",
            Self::EngineStructureDefinition => "ENGINE_STRUCTURE_N",
            Self::JsonNull => "JSON_NULL",
            Self::JsonFalse => "JSON_FALSE",
            Self::JsonTrue => "JSON_TRUE",
            Self::JsonObject => "JSON_OBJECT",
            Self::JsonArray => "JSON_ARRAY",
            Self::JsonString => "JSON_STRING",
            Self::LocationInvalid => "LOCATION_INVALID",
            Self::FunctionMacro => "__FUNCTION__",
            Self::FileMacro => "__FILE__",
            Self::LineMacro => "__LINE__",
            Self::DateMacro => "__DATE__",
            Self::TimeMacro => "__TIME__",
        }
    }

    /// Returns the upstream token code from `scriptinternal.h`.
    #[must_use]
    pub const fn upstream_token_code(self) -> u16 {
        match self {
            Self::If => 17,
            Self::Do => 52,
            Self::Else => 18,
            Self::Int => 29,
            Self::Float => 30,
            Self::String => 31,
            Self::Object => 32,
            Self::Return => 49,
            Self::While => 50,
            Self::For => 51,
            Self::Void => 53,
            Self::Case => 103,
            Self::Break => 104,
            Self::Struct => 54,
            Self::Action => 47,
            Self::Switch => 105,
            Self::Default => 106,
            Self::Include => 57,
            Self::Continue => 107,
            Self::Vector => 59,
            Self::Const => 108,
            Self::Define => 60,
            Self::ObjectSelf => 83,
            Self::ObjectInvalid => 84,
            Self::EngineNumStructuresDefinition => 61,
            Self::EngineStructureDefinition => 62,
            Self::JsonNull => 109,
            Self::JsonFalse => 110,
            Self::JsonTrue => 111,
            Self::JsonObject => 112,
            Self::JsonArray => 113,
            Self::JsonString => 114,
            Self::LocationInvalid => 115,
            Self::FunctionMacro => 117,
            Self::FileMacro => 118,
            Self::LineMacro => 119,
            Self::DateMacro => 120,
            Self::TimeMacro => 121,
        }
    }

    /// Resolves a keyword from its exact source spelling.
    #[must_use]
    pub fn from_lexeme(input: &str) -> Option<Self> {
        match input {
            "if" => Some(Self::If),
            "do" => Some(Self::Do),
            "else" => Some(Self::Else),
            "int" => Some(Self::Int),
            "float" => Some(Self::Float),
            "string" => Some(Self::String),
            "object" => Some(Self::Object),
            "return" => Some(Self::Return),
            "while" => Some(Self::While),
            "for" => Some(Self::For),
            "void" => Some(Self::Void),
            "case" => Some(Self::Case),
            "break" => Some(Self::Break),
            "struct" => Some(Self::Struct),
            "action" => Some(Self::Action),
            "switch" => Some(Self::Switch),
            "default" => Some(Self::Default),
            "#include" => Some(Self::Include),
            "continue" => Some(Self::Continue),
            "vector" => Some(Self::Vector),
            "const" => Some(Self::Const),
            "#define" => Some(Self::Define),
            "OBJECT_SELF" => Some(Self::ObjectSelf),
            "OBJECT_INVALID" => Some(Self::ObjectInvalid),
            "ENGINE_NUM_STRUCTURES" => Some(Self::EngineNumStructuresDefinition),
            "ENGINE_STRUCTURE_0" | "ENGINE_STRUCTURE_1" | "ENGINE_STRUCTURE_2"
            | "ENGINE_STRUCTURE_3" | "ENGINE_STRUCTURE_4" | "ENGINE_STRUCTURE_5"
            | "ENGINE_STRUCTURE_6" | "ENGINE_STRUCTURE_7" | "ENGINE_STRUCTURE_8"
            | "ENGINE_STRUCTURE_9" => Some(Self::EngineStructureDefinition),
            "JSON_NULL" => Some(Self::JsonNull),
            "JSON_FALSE" => Some(Self::JsonFalse),
            "JSON_TRUE" => Some(Self::JsonTrue),
            "JSON_OBJECT" => Some(Self::JsonObject),
            "JSON_ARRAY" => Some(Self::JsonArray),
            "JSON_STRING" => Some(Self::JsonString),
            "LOCATION_INVALID" => Some(Self::LocationInvalid),
            "__FUNCTION__" => Some(Self::FunctionMacro),
            "__FILE__" => Some(Self::FileMacro),
            "__LINE__" => Some(Self::LineMacro),
            "__DATE__" => Some(Self::DateMacro),
            "__TIME__" => Some(Self::TimeMacro),
            _ => None,
        }
    }
}

/// One lexical token in `NWScript` source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenKind {
    /// End of file.
    Eof,
    /// An unqualified identifier.
    Identifier,
    /// A decimal integer literal.
    Integer,
    /// A hexadecimal integer literal.
    HexInteger,
    /// A binary integer literal.
    BinaryInteger,
    /// An octal integer literal.
    OctalInteger,
    /// A floating-point literal.
    Float,
    /// A cooked string literal, including raw-string inputs.
    String,
    /// A cooked hashed string literal.
    ///
    /// The upstream lexer lowers these to `HexInteger` tokens immediately, so
    /// this variant is retained only as shared vocabulary.
    HashedString,
    /// One recognized keyword.
    Keyword(Keyword),
    /// `/`
    Divide,
    /// `&&`
    LogicalAnd,
    /// `||`
    LogicalOr,
    /// `-`
    Minus,
    /// `{`
    LeftBrace,
    /// `}`
    RightBrace,
    /// `(`
    LeftParen,
    /// `)`
    RightParen,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `>=`
    GreaterEqual,
    /// `<=`
    LessEqual,
    /// `>`
    GreaterThan,
    /// `<`
    LessThan,
    /// `!=`
    NotEqual,
    /// `==`
    EqualEqual,
    /// `+`
    Plus,
    /// `%`
    Modulus,
    /// `=`
    Assign,
    /// `|`
    InclusiveOr,
    /// `^`
    ExclusiveOr,
    /// `&`
    BooleanAnd,
    /// `<<`
    ShiftLeft,
    /// `>>`
    ShiftRight,
    /// `*`
    Multiply,
    /// `>>>`
    UnsignedShiftRight,
    /// `~`
    Tilde,
    /// `.`
    StructurePartSpecify,
    /// `!`
    BooleanNot,
    /// `[`
    LeftSquareBracket,
    /// `]`
    RightSquareBracket,
    /// `++`
    Increment,
    /// `--`
    Decrement,
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
    /// `?`
    QuestionMark,
    /// `:`
    Colon,
    /// `$`, reserved by the extended macro syntax.
    Dollar,
    /// `#` when it begins an extended attribute (`#[...]`).
    Hash,
}

/// One token plus its source span and normalized payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token {
    /// Token kind.
    pub kind: TokenKind,
    /// Source span covering the original token text.
    pub span: Span,
    /// Normalized token payload.
    pub text: String,
}

impl Token {
    /// Creates a new token.
    pub fn new(kind: TokenKind, span: Span, text: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Keyword, TokenKind};

    #[test]
    fn resolves_keyword_table_entries_from_upstream() {
        assert_eq!(Keyword::from_lexeme("#include"), Some(Keyword::Include));
        assert_eq!(
            Keyword::from_lexeme("ENGINE_STRUCTURE_3"),
            Some(Keyword::EngineStructureDefinition)
        );
        assert_eq!(Keyword::from_lexeme("__TIME__"), Some(Keyword::TimeMacro));
        assert_eq!(Keyword::from_lexeme("not_a_keyword"), None);
    }

    #[test]
    fn keyword_token_code_matches_upstream_header() {
        assert_eq!(Keyword::Include.upstream_token_code(), 57);
        assert_eq!(Keyword::JsonTrue.upstream_token_code(), 111);
        assert_eq!(Keyword::TimeMacro.upstream_token_code(), 121);
    }

    #[test]
    fn token_kind_equality_handles_keyword_payload() {
        assert_eq!(
            TokenKind::Keyword(Keyword::If),
            TokenKind::Keyword(Keyword::If)
        );
    }
}
