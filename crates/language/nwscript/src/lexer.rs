use std::{error::Error, fmt};

use crate::{
    CompilerErrorCode, Keyword, MAX_TOKEN_LENGTH, Token, TokenKind, nwscript_string_hash_bytes,
    source::{SourceFile, SourceId, Span},
};

/// A lexical error returned while scanning NWScript source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexerError {
    /// Stable upstream-aligned compiler error code.
    pub code:    CompilerErrorCode,
    /// Source span where lexing failed.
    pub span:    Span,
    /// Human-readable error message.
    pub message: String,
}

impl LexerError {
    fn new(
        code: CompilerErrorCode,
        source_id: SourceId,
        start: usize,
        end: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            span: Span::new(source_id, start, end),
            message: message.into(),
        }
    }
}

impl fmt::Display for LexerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code.code())
    }
}

impl Error for LexerError {}

/// Lexes NWScript source using the upstream compiler's token vocabulary.
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    source_id: SourceId,
    input:     &'a [u8],
    position:  usize,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer for one source file's contents.
    pub fn new(source_id: SourceId, input: &'a [u8]) -> Self {
        Self {
            source_id,
            input,
            position: 0,
        }
    }

    /// Lexes the entire input into a token vector ending with `Eof`.
    pub fn lex_all(&mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia();
            let token = self.next_token()?;
            let is_eof = token.kind == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn next_token(&mut self) -> Result<Token, LexerError> {
        if self.position >= self.input.len() {
            return Ok(Token::new(
                TokenKind::Eof,
                Span::new(self.source_id, self.position, self.position),
                "",
            ));
        }

        if self.starts_with_raw_string() {
            return self.lex_raw_string();
        }
        if self.starts_with_hashed_string() {
            return self.lex_hashed_string();
        }

        let start = self.position;
        let current = self.current_byte();
        match current {
            Some(b'0'..=b'9') => self.lex_number(),
            Some(b'.') if self.peek_byte(1).is_some_and(|next| next.is_ascii_digit()) => {
                self.lex_number()
            }
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => self.lex_identifier(),
            Some(b'#') => self.lex_hash_identifier_or_error(),
            Some(b'"') => self.lex_string(),
            Some(_) => self.lex_punctuation(start),
            None => Ok(Token::new(
                TokenKind::Eof,
                Span::new(self.source_id, self.position, self.position),
                "",
            )),
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            if self.position >= self.input.len() {
                return;
            }

            match self.current_byte() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => {
                    self.position += 1;
                }
                Some(b'/') if self.peek_byte(1) == Some(b'/') => {
                    self.position += 2;
                    while let Some(byte) = self.current_byte() {
                        if byte == b'\n' {
                            break;
                        }
                        self.position += 1;
                    }
                }
                Some(b'/') if self.peek_byte(1) == Some(b'*') => {
                    self.position += 2;
                    while self.position < self.input.len() {
                        if self.current_byte() == Some(b'*') && self.peek_byte(1) == Some(b'/') {
                            self.position += 2;
                            break;
                        }
                        self.position += 1;
                    }
                }
                _ => return,
            }
        }
    }

    fn lex_number(&mut self) -> Result<Token, LexerError> {
        let start = self.position;
        let mut text = String::new();
        let mut kind = TokenKind::Integer;

        if self.current_byte() == Some(b'.') {
            kind = TokenKind::Float;
            text.push('0');
            text.push('.');
            self.position += 1;
            self.consume_ascii_digits(&mut text);
            self.consume_float_suffix_if_present(&mut kind);
            return self.finish_token(kind, start, self.position, text);
        }

        self.consume_ascii_digits(&mut text);

        if text == "0" {
            match self.current_byte() {
                Some(b'x' | b'X') => {
                    kind = TokenKind::HexInteger;
                    if let Some(prefix) = self.bump_byte() {
                        text.push(char::from(prefix));
                    }
                    while let Some(byte) = self.current_byte() {
                        if byte.is_ascii_hexdigit() {
                            let lowered = if (b'A'..=b'F').contains(&byte) {
                                byte + 32
                            } else {
                                byte
                            };
                            text.push(char::from(lowered));
                            self.position += 1;
                        } else {
                            break;
                        }
                    }
                    return self.finish_token(kind, start, self.position, text);
                }
                Some(b'b' | b'B') => {
                    kind = TokenKind::BinaryInteger;
                    if let Some(prefix) = self.bump_byte() {
                        text.push(char::from(prefix));
                    }
                    while let Some(byte) = self.current_byte() {
                        if matches!(byte, b'0' | b'1') {
                            text.push(char::from(byte));
                            self.position += 1;
                        } else {
                            break;
                        }
                    }
                    return self.finish_token(kind, start, self.position, text);
                }
                Some(b'o' | b'O') => {
                    kind = TokenKind::OctalInteger;
                    if let Some(prefix) = self.bump_byte() {
                        text.push(char::from(prefix));
                    }
                    while let Some(byte) = self.current_byte() {
                        if (b'0'..=b'7').contains(&byte) {
                            text.push(char::from(byte));
                            self.position += 1;
                        } else {
                            break;
                        }
                    }
                    return self.finish_token(kind, start, self.position, text);
                }
                _ => {}
            }
        }

        if self.current_byte() == Some(b'.') {
            kind = TokenKind::Float;
            text.push('.');
            self.position += 1;
            self.consume_ascii_digits(&mut text);
        }

        self.consume_float_suffix_if_present(&mut kind);
        self.finish_token(kind, start, self.position, text)
    }

    fn lex_identifier(&mut self) -> Result<Token, LexerError> {
        let start = self.position;
        let mut text = String::new();
        while let Some(byte) = self.current_byte() {
            if is_identifier_continue(byte) {
                text.push(char::from(byte));
                self.position += 1;
            } else {
                break;
            }
        }
        self.finish_identifier_like_token(start, self.position, text)
    }

    fn lex_hash_identifier_or_error(&mut self) -> Result<Token, LexerError> {
        let start = self.position;
        self.position += 1;

        if !self.current_byte().is_some_and(is_identifier_start) {
            return Err(LexerError::new(
                CompilerErrorCode::EllipsisInIdentifier,
                self.source_id,
                start,
                self.position,
                "invalid preprocessor-like identifier",
            ));
        }

        let mut text = String::from("#");
        while let Some(byte) = self.current_byte() {
            if is_identifier_continue(byte) {
                text.push(char::from(byte));
                self.position += 1;
            } else {
                break;
            }
        }
        self.finish_identifier_like_token(start, self.position, text)
    }

    fn finish_identifier_like_token(
        &self,
        start: usize,
        end: usize,
        text: String,
    ) -> Result<Token, LexerError> {
        if let Some(keyword) = Keyword::from_lexeme(&text) {
            return self.finish_token(TokenKind::Keyword(keyword), start, end, text);
        }
        if text.starts_with('#') {
            return Err(LexerError::new(
                CompilerErrorCode::EllipsisInIdentifier,
                self.source_id,
                start,
                end,
                format!("unknown preprocessor-like identifier {text:?}"),
            ));
        }
        self.finish_token(TokenKind::Identifier, start, end, text)
    }

    fn lex_string(&mut self) -> Result<Token, LexerError> {
        let start = self.position;
        self.position += 1;
        let mut text = String::new();

        while let Some(byte) = self.current_byte() {
            match byte {
                b'\n' => {
                    return Err(LexerError::new(
                        CompilerErrorCode::UnterminatedStringConstant,
                        self.source_id,
                        start,
                        self.position,
                        "unterminated string constant",
                    ));
                }
                b'"' => {
                    self.position += 1;
                    return self.finish_token(TokenKind::String, start, self.position, text);
                }
                b'\\' => {
                    let next = self.peek_byte(1);
                    match next {
                        Some(b'n') => {
                            text.push('\n');
                            self.position += 2;
                        }
                        Some(b'\\') => {
                            text.push('\\');
                            self.position += 2;
                        }
                        Some(b'"') => {
                            text.push('"');
                            self.position += 2;
                        }
                        Some(b'x') => {
                            let first = self.peek_byte(2);
                            let second = self.peek_byte(3);
                            if first.is_none() || second.is_none() {
                                return Err(LexerError::new(
                                    CompilerErrorCode::UnterminatedStringConstant,
                                    self.source_id,
                                    start,
                                    self.input.len(),
                                    "unterminated hexadecimal string escape",
                                ));
                            }
                            let value = parse_upstream_hex_escape(
                                first.unwrap_or_default(),
                                second.unwrap_or_default(),
                            );
                            text.push(char::from(value));
                            self.position += 4;
                        }
                        Some(_) => {
                            self.position += 1;
                        }
                        None => {
                            return Err(LexerError::new(
                                CompilerErrorCode::UnterminatedStringConstant,
                                self.source_id,
                                start,
                                self.input.len(),
                                "unterminated string constant",
                            ));
                        }
                    }
                }
                _ => {
                    text.push(byte_to_text_char(byte));
                    self.position += 1;
                }
            }
        }

        Err(LexerError::new(
            CompilerErrorCode::UnterminatedStringConstant,
            self.source_id,
            start,
            self.input.len(),
            "unterminated string constant",
        ))
    }

    fn lex_raw_string(&mut self) -> Result<Token, LexerError> {
        let start = self.position;
        self.position += 2;
        let mut text = String::new();

        while let Some(byte) = self.current_byte() {
            if byte == b'"' {
                if self.peek_byte(1) == Some(b'"') {
                    text.push('"');
                    self.position += 2;
                    continue;
                }

                self.position += 1;
                return self.finish_token(TokenKind::String, start, self.position, text);
            }

            text.push(byte_to_text_char(byte));
            self.position += 1;
        }

        Err(LexerError::new(
            CompilerErrorCode::UnterminatedStringConstant,
            self.source_id,
            start,
            self.input.len(),
            "unterminated raw string constant",
        ))
    }

    fn lex_hashed_string(&mut self) -> Result<Token, LexerError> {
        let start = self.position;
        self.position += 2;
        let mut cooked_bytes = Vec::new();

        while let Some(byte) = self.current_byte() {
            match byte {
                b'\n' => {
                    return Err(LexerError::new(
                        CompilerErrorCode::UnterminatedStringConstant,
                        self.source_id,
                        start,
                        self.position,
                        "unterminated hashed string constant",
                    ));
                }
                b'"' => {
                    self.position += 1;
                    let lowered =
                        format!("0x{:x}", nwscript_string_hash_bytes(&cooked_bytes) as u32);
                    return self.finish_token(TokenKind::HexInteger, start, self.position, lowered);
                }
                b'\\' => {
                    let next = self.peek_byte(1);
                    match next {
                        Some(b'n') => {
                            cooked_bytes.push(b'\n');
                            self.position += 2;
                        }
                        Some(b'\\') => {
                            cooked_bytes.push(b'\\');
                            self.position += 2;
                        }
                        Some(b'"') => {
                            cooked_bytes.push(b'"');
                            self.position += 2;
                        }
                        Some(b'x') => {
                            let first = self.peek_byte(2);
                            let second = self.peek_byte(3);
                            if first.is_none() || second.is_none() {
                                return Err(LexerError::new(
                                    CompilerErrorCode::UnterminatedStringConstant,
                                    self.source_id,
                                    start,
                                    self.input.len(),
                                    "unterminated hexadecimal hashed-string escape",
                                ));
                            }
                            let value = parse_upstream_hex_escape(
                                first.unwrap_or_default(),
                                second.unwrap_or_default(),
                            );
                            cooked_bytes.push(value);
                            self.position += 4;
                        }
                        Some(_) => {
                            self.position += 1;
                        }
                        None => {
                            return Err(LexerError::new(
                                CompilerErrorCode::UnterminatedStringConstant,
                                self.source_id,
                                start,
                                self.input.len(),
                                "unterminated hashed string constant",
                            ));
                        }
                    }
                }
                _ => {
                    cooked_bytes.push(byte);
                    self.position += 1;
                }
            }
        }

        Err(LexerError::new(
            CompilerErrorCode::UnterminatedStringConstant,
            self.source_id,
            start,
            self.input.len(),
            "unterminated hashed string constant",
        ))
    }

    fn lex_punctuation(&mut self, start: usize) -> Result<Token, LexerError> {
        if self.slice_eq(start, start + 4, b">>>=") {
            self.position += 4;
            return self.finish_token(
                TokenKind::AssignUnsignedShiftRight,
                start,
                self.position,
                ">>>=".to_string(),
            );
        }

        if let Some((kind, text)) = [
            (TokenKind::UnsignedShiftRight, ">>>"),
            (TokenKind::AssignShiftRight, ">>="),
            (TokenKind::AssignShiftLeft, "<<="),
        ]
        .into_iter()
        .find(|(_, text)| self.slice_eq(start, start + text.len(), text.as_bytes()))
        {
            let width = text.len();
            self.position += width;
            return self.finish_token(kind, start, self.position, text.to_string());
        }

        if let Some((kind, text)) = [
            (TokenKind::LogicalAnd, "&&"),
            (TokenKind::LogicalOr, "||"),
            (TokenKind::GreaterEqual, ">="),
            (TokenKind::LessEqual, "<="),
            (TokenKind::NotEqual, "!="),
            (TokenKind::EqualEqual, "=="),
            (TokenKind::ShiftLeft, "<<"),
            (TokenKind::ShiftRight, ">>"),
            (TokenKind::Increment, "++"),
            (TokenKind::Decrement, "--"),
            (TokenKind::AssignMinus, "-="),
            (TokenKind::AssignPlus, "+="),
            (TokenKind::AssignMultiply, "*="),
            (TokenKind::AssignDivide, "/="),
            (TokenKind::AssignModulus, "%="),
            (TokenKind::AssignAnd, "&="),
            (TokenKind::AssignXor, "^="),
            (TokenKind::AssignOr, "|="),
        ]
        .into_iter()
        .find(|(_, text)| self.slice_eq(start, start + text.len(), text.as_bytes()))
        {
            let width = text.len();
            self.position += width;
            return self.finish_token(kind, start, self.position, text.to_string());
        }

        if let Some((kind, ch)) = self.current_byte().and_then(|byte| {
            let kind = match byte {
                b'/' => TokenKind::Divide,
                b'*' => TokenKind::Multiply,
                b'&' => TokenKind::BooleanAnd,
                b'|' => TokenKind::InclusiveOr,
                b'-' => TokenKind::Minus,
                b'{' => TokenKind::LeftBrace,
                b'}' => TokenKind::RightBrace,
                b'(' => TokenKind::LeftParen,
                b')' => TokenKind::RightParen,
                b'[' => TokenKind::LeftSquareBracket,
                b']' => TokenKind::RightSquareBracket,
                b'<' => TokenKind::LessThan,
                b'>' => TokenKind::GreaterThan,
                b'!' => TokenKind::BooleanNot,
                b'=' => TokenKind::Assign,
                b'+' => TokenKind::Plus,
                b'%' => TokenKind::Modulus,
                b';' => TokenKind::Semicolon,
                b',' => TokenKind::Comma,
                b'^' => TokenKind::ExclusiveOr,
                b'~' => TokenKind::Tilde,
                b'.' => TokenKind::StructurePartSpecify,
                b'?' => TokenKind::QuestionMark,
                b':' => TokenKind::Colon,
                _ => return None,
            };
            Some((kind, char::from(byte)))
        }) {
            self.position += 1;
            return self.finish_token(kind, start, self.position, ch.to_string());
        }

        Err(LexerError::new(
            CompilerErrorCode::UnexpectedCharacter,
            self.source_id,
            start,
            start.saturating_add(1),
            format!(
                "unexpected character {:?}",
                self.current_byte().map(char::from).unwrap_or('\0')
            ),
        ))
    }

    fn finish_token(
        &self,
        kind: TokenKind,
        start: usize,
        end: usize,
        text: String,
    ) -> Result<Token, LexerError> {
        if text.len() > MAX_TOKEN_LENGTH {
            return Err(LexerError::new(
                CompilerErrorCode::TokenTooLong,
                self.source_id,
                start,
                end,
                format!("token exceeds maximum length of {MAX_TOKEN_LENGTH} bytes"),
            ));
        }
        Ok(Token::new(
            kind,
            Span::new(self.source_id, start, end),
            text,
        ))
    }

    fn starts_with_raw_string(&self) -> bool {
        matches!(
            (self.current_byte(), self.peek_byte(1)),
            (Some(b'r' | b'R'), Some(b'"'))
        )
    }

    fn starts_with_hashed_string(&self) -> bool {
        matches!(
            (self.current_byte(), self.peek_byte(1)),
            (Some(b'h' | b'H'), Some(b'"'))
        )
    }

    fn consume_ascii_digits(&mut self, output: &mut String) {
        while let Some(byte) = self.current_byte() {
            if byte.is_ascii_digit() {
                output.push(char::from(byte));
                self.position += 1;
            } else {
                break;
            }
        }
    }

    fn consume_float_suffix_if_present(&mut self, kind: &mut TokenKind) {
        if self.current_byte() == Some(b'f') {
            *kind = TokenKind::Float;
            self.position += 1;
        }
    }

    fn current_byte(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn peek_byte(&self, ahead: usize) -> Option<u8> {
        self.input.get(self.position.saturating_add(ahead)).copied()
    }

    fn bump_byte(&mut self) -> Option<u8> {
        let byte = self.current_byte()?;
        self.position += 1;
        Some(byte)
    }

    fn slice_eq(&self, start: usize, end: usize, expected: &[u8]) -> bool {
        self.input.get(start..end) == Some(expected)
    }
}

/// Lexes the contents of one source file.
pub fn lex_source(source: &SourceFile) -> Result<Vec<Token>, LexerError> {
    Lexer::new(source.id, source.bytes()).lex_all()
}

/// Lexes a byte buffer associated with `source_id`.
pub fn lex_bytes(source_id: SourceId, input: &[u8]) -> Result<Vec<Token>, LexerError> {
    Lexer::new(source_id, input).lex_all()
}

/// Lexes a string slice associated with `source_id`.
pub fn lex_text(source_id: SourceId, input: &str) -> Result<Vec<Token>, LexerError> {
    lex_bytes(source_id, input.as_bytes())
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn parse_upstream_hex_escape(first: u8, second: u8) -> u8 {
    let first = hex_nibble(first);
    let second = hex_nibble(second);
    match (first, second) {
        (Some(high), Some(low)) => (high << 4) | low,
        (Some(value), None) => value,
        (None, _) => 0,
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some((byte - b'a') + 10),
        b'A'..=b'F' => Some((byte - b'A') + 10),
        _ => None,
    }
}

fn byte_to_text_char(byte: u8) -> char {
    char::from_u32(u32::from(byte)).unwrap_or('\0')
}

#[cfg(test)]
mod tests {
    use crate::{
        Keyword, SourceFile, SourceId, TokenKind, lex_bytes, lex_source, lex_text,
        nwscript_string_hash_bytes,
    };

    #[test]
    fn lexes_upstream_keyword_table_entries() {
        let source = SourceFile::new(
            SourceId::new(1),
            "keywords.nss",
            "if #include #define OBJECT_SELF JSON_TRUE __FILE__ ENGINE_STRUCTURE_0",
        );

        let tokens = lex_source(&source);
        let kinds = tokens.ok().map(|items| {
            items
                .into_iter()
                .map(|token| token.kind)
                .collect::<Vec<_>>()
        });

        assert_eq!(
            kinds,
            Some(vec![
                TokenKind::Keyword(Keyword::If),
                TokenKind::Keyword(Keyword::Include),
                TokenKind::Keyword(Keyword::Define),
                TokenKind::Keyword(Keyword::ObjectSelf),
                TokenKind::Keyword(Keyword::JsonTrue),
                TokenKind::Keyword(Keyword::FileMacro),
                TokenKind::Keyword(Keyword::EngineStructureDefinition),
                TokenKind::Eof,
            ])
        );
    }

    #[test]
    fn lexes_comments_numbers_and_operators() {
        let tokens = lex_text(
            SourceId::new(2),
            "// header\n0xAB 0b10 0o77 .42 5.f 6f >>= >>>= && ||",
        );

        let pairs = tokens.ok().map(|items| {
            items
                .into_iter()
                .map(|token| (token.kind, token.text))
                .collect::<Vec<_>>()
        });

        assert_eq!(
            pairs,
            Some(vec![
                (TokenKind::HexInteger, "0xab".to_string()),
                (TokenKind::BinaryInteger, "0b10".to_string()),
                (TokenKind::OctalInteger, "0o77".to_string()),
                (TokenKind::Float, "0.42".to_string()),
                (TokenKind::Float, "5.".to_string()),
                (TokenKind::Float, "6".to_string()),
                (TokenKind::AssignShiftRight, ">>=".to_string()),
                (TokenKind::AssignUnsignedShiftRight, ">>>=".to_string()),
                (TokenKind::LogicalAnd, "&&".to_string()),
                (TokenKind::LogicalOr, "||".to_string()),
                (TokenKind::Eof, "".to_string()),
            ])
        );
    }

    #[test]
    fn lexes_strings_raw_strings_and_hashed_strings() {
        let tokens = lex_text(
            SourceId::new(3),
            "\"a\\n\\\"\\\\\\x41\" r\"alpha\"\"beta\" h\"tag\\x3f\"",
        );

        let pairs = tokens.ok().map(|items| {
            items
                .into_iter()
                .map(|token| (token.kind, token.text))
                .collect::<Vec<_>>()
        });

        assert_eq!(
            pairs,
            Some(vec![
                (TokenKind::String, "a\n\"\\A".to_string()),
                (TokenKind::String, "alpha\"beta".to_string()),
                (
                    TokenKind::HexInteger,
                    format!("0x{:x}", nwscript_string_hash_bytes(b"tag?") as u32),
                ),
                (TokenKind::Eof, "".to_string()),
            ])
        );
    }

    #[test]
    fn lowers_hashed_strings_to_exact_upstream_hex_integers() {
        let tokens = lex_text(
            SourceId::new(5),
            "h\"hello\" H\"\" h\"\\\"\\n\\\\\\xFF\\x80\"",
        );
        let pairs = tokens.ok().map(|items| {
            items
                .into_iter()
                .map(|token| (token.kind, token.text))
                .collect::<Vec<_>>()
        });

        assert_eq!(
            pairs,
            Some(vec![
                (TokenKind::HexInteger, "0xf9cc2afc".to_string()),
                (TokenKind::HexInteger, "0x0".to_string()),
                (
                    TokenKind::HexInteger,
                    format!(
                        "0x{:x}",
                        nwscript_string_hash_bytes(&[b'"', b'\n', b'\\', 0xff, 0x80]) as u32
                    ),
                ),
                (TokenKind::Eof, "".to_string()),
            ])
        );
    }

    #[test]
    fn rejects_unknown_hash_prefixed_identifier_like_upstream() {
        let error = lex_text(SourceId::new(4), "#pragma").err();

        assert_eq!(
            error.map(|item| item.code),
            Some(crate::CompilerErrorCode::EllipsisInIdentifier)
        );
    }

    #[test]
    fn lexes_non_utf8_string_bytes_without_rejecting_source() {
        let tokens = lex_bytes(SourceId::new(6), b"\"a\x93\xff\"");
        let string_token = tokens.ok().and_then(|items| {
            items
                .into_iter()
                .find(|token| token.kind == TokenKind::String)
        });

        let codepoints =
            string_token.map(|token| token.text.chars().map(|ch| ch as u32).collect::<Vec<_>>());

        assert_eq!(codepoints, Some(vec![0x61, 0x93, 0xff]));
    }
}
