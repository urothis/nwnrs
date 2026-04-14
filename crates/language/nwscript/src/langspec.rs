use std::{collections::HashMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{
    CompilerErrorCode, Keyword, LexerError, ScriptResolver, SourceError, SourceFile,
    SourceLoadOptions, SourceMap, Span, Token, TokenKind,
    int_literal::{parse_wrapping_decimal_i32, parse_wrapping_prefixed_i32},
    lex_source, load_source_bundle,
};

/// Default logical script name for the builtin `NWScript` language definition.
pub const DEFAULT_LANGSPEC_SCRIPT_NAME: &str = "nwscript";

/// One builtin `NWScript` type defined by `nwscript.nss`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BuiltinType {
    /// `int`
    Int,
    /// `float`
    Float,
    /// `string`
    String,
    /// `object`
    Object,
    /// `void`
    Void,
    /// `action`
    Action,
    /// `vector`
    Vector,
    /// One engine-defined structure such as `effect` or `json`.
    EngineStructure(String),
}

/// One literal builtin value extracted from the language spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BuiltinValue {
    /// Integer literal.
    Int(i32),
    /// Floating-point literal.
    Float(f32),
    /// String literal.
    String(String),
    /// Raw object-id sentinel used for builtin object defaults such as
    /// `OBJECT_TYPE_INVALID`.
    ObjectId(i32),
    /// `OBJECT_SELF`
    ObjectSelf,
    /// `OBJECT_INVALID`
    ObjectInvalid,
    /// `LOCATION_INVALID`
    LocationInvalid,
    /// One JSON default represented in the same textual form upstream stores.
    Json(String),
    /// Vector literal.
    Vector([f32; 3]),
    /// One builtin value preserved as raw source text when this parser does not
    /// yet understand its exact typed form.
    Raw(String),
}

/// One builtin constant declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuiltinConstant {
    /// Constant name.
    pub name:  String,
    /// Constant type.
    pub ty:    BuiltinType,
    /// Constant value.
    pub value: BuiltinValue,
}

/// One builtin function parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuiltinParameter {
    /// Parameter name.
    pub name:    String,
    /// Parameter type.
    pub ty:      BuiltinType,
    /// Optional default value.
    pub default: Option<BuiltinValue>,
}

/// One builtin function declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuiltinFunction {
    /// Function name.
    pub name:        String,
    /// Return type.
    pub return_type: BuiltinType,
    /// Parameters in declaration order.
    pub parameters:  Vec<BuiltinParameter>,
}

/// Parsed builtin declarations from `nwscript.nss`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LangSpec {
    /// Number declared by `ENGINE_NUM_STRUCTURES`.
    pub engine_num_structures: usize,
    /// Engine structure names in index order.
    pub engine_structures:     Vec<String>,
    /// Builtin constants in declaration order.
    pub constants:             Vec<BuiltinConstant>,
    /// Builtin functions in declaration order.
    pub functions:             Vec<BuiltinFunction>,
}

/// Errors returned while bootstrapping the builtin language spec.
#[derive(Debug)]
pub enum LangSpecError {
    /// Source loading failure.
    Source(SourceError),
    /// Lexing failure.
    Lex(LexerError),
    /// The language specification text was malformed.
    Parse {
        /// Upstream-aligned compiler error code.
        code:    CompilerErrorCode,
        /// Human-readable message.
        message: String,
    },
}

impl LangSpecError {
    fn parse(message: impl Into<String>) -> Self {
        Self::Parse {
            code:    CompilerErrorCode::ParsingIdentifierList,
            message: message.into(),
        }
    }
}

impl fmt::Display for LangSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => error.fmt(f),
            Self::Lex(error) => error.fmt(f),
            Self::Parse {
                code,
                message,
            } => write!(f, "{message} ({})", code.code()),
        }
    }
}

impl Error for LangSpecError {}

impl From<SourceError> for LangSpecError {
    fn from(value: SourceError) -> Self {
        Self::Source(value)
    }
}

impl From<LexerError> for LangSpecError {
    fn from(value: LexerError) -> Self {
        Self::Lex(value)
    }
}

/// Loads `nwscript.nss` through a source resolver and parses the builtin
/// declarations.
pub fn load_langspec<R: ScriptResolver + ?Sized>(
    resolver: &R,
    script_name: &str,
    options: SourceLoadOptions,
) -> Result<LangSpec, LangSpecError> {
    let bundle =
        load_source_bundle(resolver, script_name, options).map_err(|error| match error {
            crate::PreprocessError::Source(source) => LangSpecError::Source(source),
            crate::PreprocessError::Lex(lex) => LangSpecError::Lex(lex),
        })?;
    parse_langspec_from_source_map(&bundle.source_map, bundle.root_id)
}

/// Parses builtin declarations from one already-loaded source file.
pub fn parse_langspec(source_name: &str, input: &str) -> Result<LangSpec, LangSpecError> {
    parse_langspec_bytes(source_name, input.as_bytes())
}

/// Parses builtin declarations from one already-loaded byte buffer.
pub fn parse_langspec_bytes(source_name: &str, input: &[u8]) -> Result<LangSpec, LangSpecError> {
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file(source_name, input);
    parse_langspec_from_source_map(&source_map, root_id)
}

/// Parses builtin declarations from `root_id` in `source_map`.
pub fn parse_langspec_from_source_map(
    source_map: &SourceMap,
    root_id: crate::SourceId,
) -> Result<LangSpec, LangSpecError> {
    let source = source_map
        .get(root_id)
        .ok_or_else(|| LangSpecError::parse("missing root source file for langspec parse"))?;
    let tokens = lex_source(source)?;
    LangSpecParser::new(source, tokens).parse()
}

struct LangSpecParser<'a> {
    source:                &'a SourceFile,
    tokens:                Vec<Token>,
    position:              usize,
    engine_num_structures: usize,
    engine_structures:     Vec<String>,
    constants:             Vec<BuiltinConstant>,
    functions:             Vec<BuiltinFunction>,
    constant_values:       HashMap<String, BuiltinValue>,
}

impl<'a> LangSpecParser<'a> {
    fn new(source: &'a SourceFile, tokens: Vec<Token>) -> Self {
        Self {
            source,
            tokens,
            position: 0,
            engine_num_structures: 0,
            engine_structures: Vec::new(),
            constants: Vec::new(),
            functions: Vec::new(),
            constant_values: HashMap::new(),
        }
    }

    fn parse(mut self) -> Result<LangSpec, LangSpecError> {
        while !self.at_eof() {
            if self.matches_keyword(Keyword::Define) {
                self.parse_define()?;
            } else {
                self.parse_declaration()?;
            }
        }

        Ok(LangSpec {
            engine_num_structures: self.engine_num_structures,
            engine_structures:     self.engine_structures,
            constants:             self.constants,
            functions:             self.functions,
        })
    }

    fn parse_define(&mut self) -> Result<(), LangSpecError> {
        self.expect_keyword(Keyword::Define)?;
        if self.matches_keyword(Keyword::EngineNumStructuresDefinition) {
            self.advance();
            let value = self.expect_integer_literal()?;
            self.engine_num_structures =
                usize::try_from(value).map_err(|error| LangSpecError::parse(error.to_string()))?;
            return Ok(());
        }

        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF after #define"))?;
        let define_line = self.line_number_for_token(&token);
        let index = match &token.kind {
            TokenKind::Identifier | TokenKind::Keyword(Keyword::EngineStructureDefinition) => {
                let Some(index) = parse_engine_structure_define_index(&token.text) else {
                    self.skip_line(define_line);
                    return Ok(());
                };
                index
            }
            _ => {
                self.skip_line(define_line);
                return Ok(());
            }
        };

        let structure_name = self.expect_identifier_like_name()?;
        if self.engine_structures.len() <= index {
            self.engine_structures.resize(index + 1, String::new());
        }
        if let Some(slot) = self.engine_structures.get_mut(index) {
            *slot = structure_name;
        }
        Ok(())
    }

    fn parse_declaration(&mut self) -> Result<(), LangSpecError> {
        let ty = self.parse_type()?;
        let name = self.expect_identifier_like_name()?;

        if self.matches_kind(&TokenKind::Assign) {
            self.advance();
            let value = self.parse_value_for_type_or_raw(&ty, &[TokenKind::Semicolon])?;
            self.expect_kind(TokenKind::Semicolon)?;
            self.constant_values.insert(name.clone(), value.clone());
            self.constants.push(BuiltinConstant {
                name,
                ty,
                value,
            });
            return Ok(());
        }

        self.expect_kind(TokenKind::LeftParen)?;
        let parameters = self.parse_parameters()?;
        self.expect_kind(TokenKind::RightParen)?;
        self.expect_kind(TokenKind::Semicolon)?;
        self.functions.push(BuiltinFunction {
            name,
            return_type: ty,
            parameters,
        });
        Ok(())
    }

    fn parse_parameters(&mut self) -> Result<Vec<BuiltinParameter>, LangSpecError> {
        let mut parameters = Vec::new();
        while !self.matches_kind(&TokenKind::RightParen) {
            let ty = self.parse_type()?;
            let name = self.expect_identifier_like_name()?;
            let default =
                if self.matches_kind(&TokenKind::Assign) {
                    self.advance();
                    Some(self.parse_value_for_type_or_raw(
                        &ty,
                        &[TokenKind::Comma, TokenKind::RightParen],
                    )?)
                } else {
                    None
                };
            parameters.push(BuiltinParameter {
                name,
                ty,
                default,
            });

            if self.matches_kind(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(parameters)
    }

    fn parse_type(&mut self) -> Result<BuiltinType, LangSpecError> {
        let token = self
            .peek()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing type"))?;
        let parsed = match &token.kind {
            TokenKind::Keyword(Keyword::Int) => Some(BuiltinType::Int),
            TokenKind::Keyword(Keyword::Float) => Some(BuiltinType::Float),
            TokenKind::Keyword(Keyword::String) => Some(BuiltinType::String),
            TokenKind::Keyword(Keyword::Object) => Some(BuiltinType::Object),
            TokenKind::Keyword(Keyword::Void) => Some(BuiltinType::Void),
            TokenKind::Keyword(Keyword::Action) => Some(BuiltinType::Action),
            TokenKind::Keyword(Keyword::Vector) => Some(BuiltinType::Vector),
            TokenKind::Identifier => Some(BuiltinType::EngineStructure(token.text.clone())),
            _ => None,
        };
        if let Some(parsed) = parsed {
            self.advance();
            Ok(parsed)
        } else {
            Err(LangSpecError::parse(format!(
                "unsupported builtin type token {:?}",
                token.kind
            )))
        }
    }

    fn parse_value_for_type(&mut self, ty: &BuiltinType) -> Result<BuiltinValue, LangSpecError> {
        match ty {
            BuiltinType::Int => self.parse_int_like_value().map(BuiltinValue::Int),
            BuiltinType::Float => self.parse_float_like_value().map(BuiltinValue::Float),
            BuiltinType::String => self.parse_string_like_value().map(BuiltinValue::String),
            BuiltinType::Object => self.parse_object_default(),
            BuiltinType::Vector => self.parse_vector_value(),
            BuiltinType::EngineStructure(name) if name.eq_ignore_ascii_case("location") => {
                self.parse_location_default()
            }
            BuiltinType::EngineStructure(name) if name.eq_ignore_ascii_case("json") => {
                self.parse_json_default()
            }
            _ => Err(LangSpecError::parse(format!(
                "unsupported builtin default value for type {ty:?}"
            ))),
        }
    }

    fn parse_value_for_type_or_raw(
        &mut self,
        ty: &BuiltinType,
        terminators: &[TokenKind],
    ) -> Result<BuiltinValue, LangSpecError> {
        let checkpoint = self.position;
        match self.parse_value_for_type(ty) {
            Ok(value) => Ok(value),
            Err(_error) => {
                self.position = checkpoint;
                self.parse_raw_value_until(terminators)
                    .map(BuiltinValue::Raw)
            }
        }
    }

    fn parse_raw_value_until(
        &mut self,
        terminators: &[TokenKind],
    ) -> Result<String, LangSpecError> {
        let first = self
            .peek()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing builtin value"))?;
        let start = first.span.start;
        let mut end = first.span.end;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        let mut brace_depth = 0usize;
        let mut consumed = false;

        while let Some(token) = self.peek() {
            if paren_depth == 0
                && bracket_depth == 0
                && brace_depth == 0
                && terminators.iter().any(|kind| kind == &token.kind)
            {
                break;
            }

            consumed = true;
            end = token.span.end;
            match token.kind {
                TokenKind::LeftParen => paren_depth += 1,
                TokenKind::RightParen => paren_depth = paren_depth.saturating_sub(1),
                TokenKind::LeftSquareBracket => bracket_depth += 1,
                TokenKind::RightSquareBracket => bracket_depth = bracket_depth.saturating_sub(1),
                TokenKind::LeftBrace => brace_depth += 1,
                TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }

        if !consumed {
            return Err(LangSpecError::parse("missing builtin value"));
        }

        let span = Span::new(self.source.id, start, end);
        let raw = self
            .source
            .span_text(span)
            .ok_or_else(|| LangSpecError::parse("invalid raw builtin value span"))?
            .trim()
            .to_string();

        if raw.is_empty() {
            return Err(LangSpecError::parse("missing builtin value"));
        }

        Ok(raw)
    }

    fn parse_int_like_value(&mut self) -> Result<i32, LangSpecError> {
        let sign = if self.matches_kind(&TokenKind::Minus) {
            self.advance();
            -1
        } else {
            1
        };

        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing integer value"))?;
        let value = match token.kind {
            TokenKind::Integer => parse_wrapping_decimal_i32(&token.text).map_err(|_error| {
                LangSpecError::parse(format!("invalid integer literal {:?}", token.text))
            })?,
            TokenKind::HexInteger => parse_prefixed_i32(&token.text, 16)?,
            TokenKind::BinaryInteger => parse_prefixed_i32(&token.text, 2)?,
            TokenKind::OctalInteger => parse_prefixed_i32(&token.text, 8)?,
            TokenKind::Identifier => match self.constant_values.get(&token.text) {
                Some(BuiltinValue::Int(value)) => *value,
                Some(_) => {
                    return Err(LangSpecError::parse(format!(
                        "constant {:?} is not an integer",
                        token.text
                    )));
                }
                None => {
                    return Err(LangSpecError::parse(format!(
                        "unknown integer constant reference {:?}",
                        token.text
                    )));
                }
            },
            _ => {
                return Err(LangSpecError::parse(format!(
                    "unsupported integer literal token {:?}",
                    token.kind
                )));
            }
        };
        Ok(sign * value)
    }

    #[allow(clippy::cast_precision_loss)]
    fn parse_float_like_value(&mut self) -> Result<f32, LangSpecError> {
        let sign = if self.matches_kind(&TokenKind::Minus) {
            self.advance();
            -1.0f32
        } else {
            1.0f32
        };

        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing float value"))?;
        let value = match token.kind {
            TokenKind::Float => token.text.parse::<f32>().map_err(|error| {
                LangSpecError::parse(format!("invalid float literal {:?}: {error}", token.text))
            })?,
            TokenKind::Integer => token.text.parse::<f32>().map_err(|error| {
                LangSpecError::parse(format!("invalid numeric literal {:?}: {error}", token.text))
            })?,
            TokenKind::Identifier => match self.constant_values.get(&token.text) {
                Some(BuiltinValue::Float(value)) => *value,
                Some(BuiltinValue::Int(value)) => *value as f32,
                Some(_) => {
                    return Err(LangSpecError::parse(format!(
                        "constant {:?} is not numeric",
                        token.text
                    )));
                }
                None => {
                    return Err(LangSpecError::parse(format!(
                        "unknown numeric constant reference {:?}",
                        token.text
                    )));
                }
            },
            _ => {
                return Err(LangSpecError::parse(format!(
                    "unsupported float literal token {:?}",
                    token.kind
                )));
            }
        };
        Ok(sign * value)
    }

    fn parse_string_like_value(&mut self) -> Result<String, LangSpecError> {
        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing string value"))?;
        match token.kind {
            TokenKind::String => Ok(token.text),
            TokenKind::Identifier => match self.constant_values.get(&token.text) {
                Some(BuiltinValue::String(value)) => Ok(value.clone()),
                Some(_) => Err(LangSpecError::parse(format!(
                    "constant {:?} is not a string",
                    token.text
                ))),
                None => Err(LangSpecError::parse(format!(
                    "unknown string constant reference {:?}",
                    token.text
                ))),
            },
            _ => Err(LangSpecError::parse(format!(
                "unsupported string literal token {:?}",
                token.kind
            ))),
        }
    }

    fn parse_object_default(&mut self) -> Result<BuiltinValue, LangSpecError> {
        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing object default"))?;
        match token.kind {
            TokenKind::Keyword(Keyword::ObjectSelf) => Ok(BuiltinValue::ObjectSelf),
            TokenKind::Keyword(Keyword::ObjectInvalid) => Ok(BuiltinValue::ObjectInvalid),
            TokenKind::Identifier => match self.constant_values.get(&token.text) {
                Some(BuiltinValue::Int(value)) => Ok(BuiltinValue::ObjectId(*value)),
                Some(BuiltinValue::ObjectSelf) => Ok(BuiltinValue::ObjectSelf),
                Some(BuiltinValue::ObjectInvalid) => Ok(BuiltinValue::ObjectInvalid),
                Some(_) => Err(LangSpecError::parse(format!(
                    "constant {:?} is not an object default",
                    token.text
                ))),
                None => Err(LangSpecError::parse(format!(
                    "unknown object constant reference {:?}",
                    token.text
                ))),
            },
            _ => Err(LangSpecError::parse(format!(
                "unsupported object default token {:?}",
                token.kind
            ))),
        }
    }

    fn parse_location_default(&mut self) -> Result<BuiltinValue, LangSpecError> {
        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing location default"))?;
        match token.kind {
            TokenKind::Keyword(Keyword::LocationInvalid) => Ok(BuiltinValue::LocationInvalid),
            _ => Err(LangSpecError::parse(format!(
                "unsupported location default token {:?}",
                token.kind
            ))),
        }
    }

    fn parse_json_default(&mut self) -> Result<BuiltinValue, LangSpecError> {
        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing json default"))?;
        match token.kind {
            TokenKind::Keyword(Keyword::JsonNull) => Ok(BuiltinValue::Json("null".to_string())),
            TokenKind::Keyword(Keyword::JsonFalse) => Ok(BuiltinValue::Json("false".to_string())),
            TokenKind::Keyword(Keyword::JsonTrue) => Ok(BuiltinValue::Json("true".to_string())),
            TokenKind::Keyword(Keyword::JsonObject) => Ok(BuiltinValue::Json("{}".to_string())),
            TokenKind::Keyword(Keyword::JsonArray) => Ok(BuiltinValue::Json("[]".to_string())),
            TokenKind::Keyword(Keyword::JsonString) => Ok(BuiltinValue::Json("\"\"".to_string())),
            _ => Err(LangSpecError::parse(format!(
                "unsupported json default token {:?}",
                token.kind
            ))),
        }
    }

    fn parse_vector_value(&mut self) -> Result<BuiltinValue, LangSpecError> {
        self.expect_kind(TokenKind::LeftSquareBracket)?;
        let x = self.parse_float_like_value()?;
        self.expect_kind(TokenKind::Comma)?;
        let y = self.parse_float_like_value()?;
        self.expect_kind(TokenKind::Comma)?;
        let z = self.parse_float_like_value()?;
        self.expect_kind(TokenKind::RightSquareBracket)?;
        Ok(BuiltinValue::Vector([x, y, z]))
    }

    fn expect_integer_literal(&mut self) -> Result<i32, LangSpecError> {
        self.parse_int_like_value()
    }

    fn expect_identifier_like_name(&mut self) -> Result<String, LangSpecError> {
        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing identifier"))?;
        match token.kind {
            TokenKind::Identifier | TokenKind::Keyword(Keyword::EngineStructureDefinition) => {
                Ok(token.text)
            }
            _ => Err(LangSpecError::parse(format!(
                "expected identifier, found {:?}",
                token.kind
            ))),
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> Result<(), LangSpecError> {
        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing keyword"))?;
        if token.kind == TokenKind::Keyword(keyword) {
            Ok(())
        } else {
            Err(LangSpecError::parse(format!(
                "expected keyword {:?}, found {:?}",
                keyword, token.kind
            )))
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn expect_kind(&mut self, kind: TokenKind) -> Result<(), LangSpecError> {
        let token = self
            .advance()
            .ok_or_else(|| LangSpecError::parse("unexpected EOF while parsing token"))?;
        if token.kind == kind {
            Ok(())
        } else {
            Err(LangSpecError::parse(format!(
                "expected token {:?}, found {:?}",
                kind, token.kind
            )))
        }
    }

    fn matches_keyword(&self, keyword: Keyword) -> bool {
        self.peek()
            .is_some_and(|token| token.kind == TokenKind::Keyword(keyword))
    }

    fn matches_kind(&self, kind: &TokenKind) -> bool {
        self.peek().is_some_and(|token| &token.kind == kind)
    }

    fn at_eof(&self) -> bool {
        self.peek().is_none_or(|token| token.kind == TokenKind::Eof)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned()?;
        self.position += 1;
        Some(token)
    }

    fn line_number_for_token(&self, token: &Token) -> Option<usize> {
        self.source
            .location(token.span.start)
            .map(|location| location.line)
    }

    fn skip_line(&mut self, line: Option<usize>) {
        let Some(line) = line else {
            return;
        };

        while let Some(token) = self.peek() {
            if token.kind == TokenKind::Eof {
                break;
            }
            if self.line_number_for_token(token) != Some(line) {
                break;
            }
            self.position += 1;
        }
    }
}

fn parse_engine_structure_define_index(input: &str) -> Option<usize> {
    input
        .strip_prefix("ENGINE_STRUCTURE_")
        .and_then(|value| value.parse::<usize>().ok())
}

fn parse_prefixed_i32(input: &str, radix: u32) -> Result<i32, LangSpecError> {
    parse_wrapping_prefixed_i32(input, radix)
        .map_err(|_error| LangSpecError::parse(format!("invalid integer literal {input:?}")))
}

#[cfg(test)]
mod tests {
    use crate::{
        BuiltinType, BuiltinValue, DEFAULT_LANGSPEC_SCRIPT_NAME, InMemoryScriptResolver,
        SourceLoadOptions, load_langspec, parse_langspec,
    };

    #[test]
    fn parses_engine_structures_constants_and_functions() {
        let spec = parse_langspec(
            "nwscript.nss",
            r#"
#define ENGINE_NUM_STRUCTURES 3
#define ENGINE_STRUCTURE_0 effect
#define ENGINE_STRUCTURE_1 location
#define ENGINE_STRUCTURE_2 json

int TRUE = 1;
int FALSE = 0;
int OBJECT_TYPE_INVALID = 32767;
float PI = 3.141592;
string HELLO = "hello";

void TestDefaults(
    int bEnabled = FALSE,
    float fRadians = PI,
    string sLabel = HELLO,
    object oTarget = OBJECT_SELF,
    object oTokenTarget = OBJECT_TYPE_INVALID,
    location lWhere = LOCATION_INVALID,
    json jData = JSON_OBJECT,
    vector vPos = [1.0, 2.0, 3.0]
);

effect EffectDamage(int nAmount);
"#,
        );

        let spec = spec.ok();
        assert_eq!(
            spec.as_ref().map(|spec| spec.engine_structures.clone()),
            Some(vec![
                "effect".to_string(),
                "location".to_string(),
                "json".to_string()
            ])
        );
        assert_eq!(spec.as_ref().map(|spec| spec.constants.len()), Some(5));
        assert_eq!(spec.as_ref().map(|spec| spec.functions.len()), Some(2));

        let defaults = spec
            .as_ref()
            .and_then(|spec| {
                spec.functions
                    .iter()
                    .find(|function| function.name == "TestDefaults")
            })
            .map(|function| {
                function
                    .parameters
                    .iter()
                    .map(|param| param.default.clone())
                    .collect::<Vec<_>>()
            });

        assert_eq!(
            defaults,
            Some(vec![
                Some(BuiltinValue::Int(0)),
                Some(BuiltinValue::Float(3_141_592.0_f32 / 1_000_000.0_f32)),
                Some(BuiltinValue::String("hello".to_string())),
                Some(BuiltinValue::ObjectSelf),
                Some(BuiltinValue::ObjectId(32767)),
                Some(BuiltinValue::LocationInvalid),
                Some(BuiltinValue::Json("{}".to_string())),
                Some(BuiltinValue::Vector([1.0, 2.0, 3.0])),
            ])
        );

        let return_type = spec
            .as_ref()
            .and_then(|spec| {
                spec.functions
                    .iter()
                    .find(|function| function.name == "EffectDamage")
            })
            .map(|function| function.return_type.clone());
        assert_eq!(
            return_type,
            Some(BuiltinType::EngineStructure("effect".to_string()))
        );
    }

    #[test]
    fn loads_langspec_through_resolver() {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            DEFAULT_LANGSPEC_SCRIPT_NAME,
            r#"
#define ENGINE_NUM_STRUCTURES 1
#define ENGINE_STRUCTURE_0 effect
int TRUE = 1;
effect EffectFoo(int bValue = TRUE);
"#,
        );

        let spec = load_langspec(
            &resolver,
            DEFAULT_LANGSPEC_SCRIPT_NAME,
            SourceLoadOptions::default(),
        );

        assert_eq!(spec.ok().map(|spec| spec.functions.len()), Some(1));
    }

    #[test]
    fn ignores_unknown_define_lines() {
        let spec = parse_langspec(
            "nwscript.nss",
            r#"
#define ENGINE_NUM_STRUCTURES 1
#define ENGINE_STRUCTURE_0 effect
#define FUTURE_FEATURE (1 << 5)
#define YET_ANOTHER_FEATURE SOME_IDENTIFIER

effect EffectDamage(int nAmount);
"#,
        )
        .expect("langspec should parse");

        assert_eq!(spec.engine_structures, vec!["effect".to_string()]);
        assert_eq!(spec.functions.len(), 1);
        assert_eq!(
            spec.functions
                .first()
                .expect("EffectDamage should be present")
                .return_type,
            BuiltinType::EngineStructure("effect".to_string())
        );
    }

    #[test]
    fn accepts_identifier_typed_builtins_before_structure_defines() {
        let spec = parse_langspec(
            "nwscript.nss",
            r#"
#define ENGINE_NUM_STRUCTURES 1

json JsonObject();

#define ENGINE_STRUCTURE_0 json
"#,
        )
        .expect("langspec should parse");

        assert_eq!(spec.functions.len(), 1);
        assert_eq!(
            spec.functions
                .first()
                .expect("JsonObject should be present")
                .return_type,
            BuiltinType::EngineStructure("json".to_string())
        );
        assert_eq!(spec.engine_structures, vec!["json".to_string()]);
    }

    #[test]
    fn preserves_unknown_builtin_values_as_raw_text() {
        let spec = parse_langspec(
            "nwscript.nss",
            r#"
#define ENGINE_NUM_STRUCTURES 2
#define ENGINE_STRUCTURE_0 effect
#define ENGINE_STRUCTURE_1 json

json JSON_DYNAMIC = JsonParse("{\"enabled\":true}");
void TestDefaults(
    json jData = JsonParse("{}"),
    effect eDamage = EffectDamage(5)
);
"#,
        )
        .expect("langspec should parse");

        assert_eq!(
            spec.constants
                .iter()
                .find(|constant| constant.name == "JSON_DYNAMIC")
                .map(|constant| constant.value.clone()),
            Some(BuiltinValue::Raw(
                "JsonParse(\"{\\\"enabled\\\":true}\")".to_string()
            ))
        );

        let defaults = spec
            .functions
            .iter()
            .find(|function| function.name == "TestDefaults")
            .expect("TestDefaults should exist")
            .parameters
            .iter()
            .map(|parameter| parameter.default.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            defaults,
            vec![
                Some(BuiltinValue::Raw("JsonParse(\"{}\")".to_string())),
                Some(BuiltinValue::Raw("EffectDamage(5)".to_string())),
            ]
        );
    }
}
