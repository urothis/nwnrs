use super::*;

impl Parser<'_> {
    pub(super) fn parse_enum_declaration(&mut self) -> Result<EnumDecl, ParserError> {
        let enum_token =
            self.consume_identifier(CompilerErrorCode::InvalidEnumDeclaration, "expected enum")?;
        let name = self.consume_identifier(
            CompilerErrorCode::InvalidEnumDeclaration,
            "expected enum name",
        )?;
        let backing = if self.matches_kind(&TokenKind::Colon) {
            self.advance();
            let backing = self.advance_required(
                CompilerErrorCode::InvalidEnumBackingType,
                "expected int or string after :",
            )?;
            match backing.kind {
                TokenKind::Keyword(Keyword::Int) => EnumBackingType::Int,
                TokenKind::Keyword(Keyword::String) => EnumBackingType::String,
                _ => {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidEnumBackingType,
                        backing.span,
                        "enum backing type must be int or string",
                    ));
                }
            }
        } else {
            EnumBackingType::Int
        };
        self.consume_kind(
            TokenKind::LeftBrace,
            CompilerErrorCode::InvalidEnumDeclaration,
            "expected { after enum name",
        )?;

        let mut variants = Vec::new();
        while !self.matches_kind(&TokenKind::RightBrace) && !self.at_eof() {
            let attribute_start = self.peek().map(|token| token.span);
            let mut is_default = false;
            let mut aliases = Vec::new();
            while self.matches_kind(&TokenKind::Hash) {
                let (attribute, argument, span) = self.parse_enum_attribute()?;
                match attribute.as_str() {
                    "default" => {
                        if argument.is_some() {
                            return Err(ParserError::new(
                                CompilerErrorCode::InvalidEnumDeclaration,
                                span,
                                "#[default] does not accept an argument",
                            ));
                        }
                        if is_default {
                            return Err(ParserError::new(
                                CompilerErrorCode::MultipleEnumDefaults,
                                span,
                                "enum variant repeats #[default]",
                            ));
                        }
                        is_default = true;
                    }
                    "alias" => {
                        let alias = argument.ok_or_else(|| {
                            ParserError::new(
                                CompilerErrorCode::InvalidEnumDeclaration,
                                span,
                                "#[alias(...)] requires a global identifier",
                            )
                        })?;
                        aliases.push(alias);
                    }
                    _ => {
                        return Err(ParserError::new(
                            CompilerErrorCode::InvalidEnumDeclaration,
                            span,
                            format!("unsupported enum variant attribute #[{attribute}]"),
                        ));
                    }
                }
            }

            let variant = self.consume_identifier(
                CompilerErrorCode::InvalidEnumDeclaration,
                "expected enum variant name",
            )?;
            let value = if self.matches_kind(&TokenKind::Assign) {
                self.advance();
                Some(self.parse_expression()?)
            } else {
                None
            };
            let end_span = value.as_ref().map_or(variant.span, |value| value.span);
            variants.push(EnumVariantDecl {
                span: attribute_start.map_or_else(
                    || join_spans(variant.span, end_span),
                    |start| join_spans(start, end_span),
                ),
                name: variant.text,
                value,
                is_default,
                aliases,
            });

            if self.matches_kind(&TokenKind::Comma) {
                self.advance();
            } else if !self.matches_kind(&TokenKind::RightBrace) {
                return Err(self.error_here(
                    CompilerErrorCode::InvalidEnumDeclaration,
                    "expected , or } after enum variant",
                ));
            }
        }
        let right = self.consume_kind(
            TokenKind::RightBrace,
            CompilerErrorCode::InvalidEnumDeclaration,
            "expected } after enum variants",
        )?;
        let end = if self.matches_kind(&TokenKind::Semicolon) {
            self.advance_required(
                CompilerErrorCode::InvalidEnumDeclaration,
                "expected optional enum semicolon",
            )?
        } else {
            right
        };
        Ok(EnumDecl {
            span: join_spans(enum_token.span, end.span),
            name: name.text,
            backing,
            variants,
        })
    }

    pub(super) fn parse_enum_attribute(
        &mut self,
    ) -> Result<(String, Option<NamedItem>, Span), ParserError> {
        let hash = self.consume_kind(
            TokenKind::Hash,
            CompilerErrorCode::InvalidEnumDeclaration,
            "expected #",
        )?;
        self.consume_kind(
            TokenKind::LeftSquareBracket,
            CompilerErrorCode::InvalidEnumDeclaration,
            "expected [ after #",
        )?;
        let attribute = match self.peek() {
            Some(Token {
                kind: TokenKind::Identifier | TokenKind::Keyword(Keyword::Default),
                ..
            }) => self.advance_required(
                CompilerErrorCode::InvalidEnumDeclaration,
                "expected enum attribute name",
            )?,
            _ => {
                return Err(self.error_here(
                    CompilerErrorCode::InvalidEnumDeclaration,
                    "expected enum attribute name",
                ));
            }
        };
        let argument = if self.matches_kind(&TokenKind::LeftParen) {
            self.advance();
            let argument = self.consume_identifier(
                CompilerErrorCode::InvalidEnumDeclaration,
                "expected identifier in enum attribute",
            )?;
            self.consume_kind(
                TokenKind::RightParen,
                CompilerErrorCode::InvalidEnumDeclaration,
                "expected ) after enum attribute argument",
            )?;
            Some(NamedItem {
                span: argument.span,
                name: argument.text,
            })
        } else {
            None
        };
        let right = self.consume_kind(
            TokenKind::RightSquareBracket,
            CompilerErrorCode::InvalidEnumDeclaration,
            "expected ] after enum attribute",
        )?;
        Ok((attribute.text, argument, join_spans(hash.span, right.span)))
    }

    pub(super) fn parse_type_alias(&mut self) -> Result<TypeAliasDecl, ParserError> {
        let type_token =
            self.consume_identifier(CompilerErrorCode::InvalidTypeAlias, "expected type")?;
        let name = self.consume_identifier(
            CompilerErrorCode::InvalidTypeAlias,
            "expected type alias name",
        )?;
        self.consume_kind(
            TokenKind::Assign,
            CompilerErrorCode::InvalidTypeAlias,
            "expected = after type alias name",
        )?;
        let target = self.parse_non_void_type_specifier()?;
        if target.is_const {
            return Err(ParserError::new(
                CompilerErrorCode::InvalidTypeAlias,
                target.span,
                "type aliases cannot contain the const declaration modifier",
            ));
        }
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::InvalidTypeAlias,
            "expected ; after type alias",
        )?;
        Ok(TypeAliasDecl {
            span: join_spans(type_token.span, semicolon.span),
            name: name.text,
            target,
        })
    }

    pub(super) fn parse_static_assert(&mut self) -> Result<StaticAssertDecl, ParserError> {
        let assertion = self.consume_identifier(
            CompilerErrorCode::StaticAssertionFailed,
            "expected static_assert",
        )?;
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::StaticAssertionFailed,
            "expected ( after static_assert",
        )?;
        let condition = self.parse_expression()?;
        let message = if self.matches_kind(&TokenKind::Comma) {
            self.advance();
            let message = self.consume_string(
                CompilerErrorCode::StaticAssertionFailed,
                "static_assert message must be a string literal",
            )?;
            Some(crate::ScriptString::from_lexed_text(&message.text))
        } else {
            None
        };
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::StaticAssertionFailed,
            "expected ) after static_assert",
        )?;
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::StaticAssertionFailed,
            "expected ; after static_assert",
        )?;
        Ok(StaticAssertDecl {
            span: join_spans(assertion.span, semicolon.span),
            condition,
            message,
        })
    }

    pub(super) fn parse_include_directive(&mut self) -> Result<IncludeDirective, ParserError> {
        let include = self.consume_keyword(
            Keyword::Include,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected #include",
        )?;
        let path = self.consume_string(
            CompilerErrorCode::UnknownStateInCompiler,
            "expected string literal after #include",
        )?;
        Ok(IncludeDirective {
            span: join_spans(include.span, path.span),
            path: path.text,
        })
    }

    pub(super) fn parse_top_level_item(&mut self) -> Result<TopLevelItem, ParserError> {
        let ty = self.parse_any_type_specifier()?;

        if matches!(ty.kind, TypeKind::Struct(_)) && self.matches_kind(&TokenKind::LeftBrace) {
            return self.parse_struct_definition(&ty).map(TopLevelItem::Struct);
        }

        let name = self.consume_identifier(
            CompilerErrorCode::FunctionDefinitionMissingName,
            "expected identifier after type specifier",
        )?;

        if self.matches_kind(&TokenKind::LeftParen) {
            return self
                .parse_function_declaration_or_definition(ty, name)
                .map(TopLevelItem::Function);
        }

        let declaration = self.parse_declaration_after_name(ty, name)?;
        Ok(TopLevelItem::Global(declaration))
    }

    pub(super) fn parse_struct_definition(
        &mut self,
        ty: &TypeSpec,
    ) -> Result<StructDecl, ParserError> {
        let name = match &ty.kind {
            TypeKind::Struct(name) => name.clone(),
            _ => {
                return Err(ParserError::new(
                    CompilerErrorCode::BadTypeSpecifier,
                    ty.span,
                    "struct definition requires a named struct type specifier",
                ));
            }
        };

        self.consume_kind(
            TokenKind::LeftBrace,
            CompilerErrorCode::BadTypeSpecifier,
            "expected { after struct name",
        )?;

        let mut fields = Vec::new();
        while !self.matches_kind(&TokenKind::RightBrace) && !self.at_eof() {
            let field_type = self.parse_non_void_type_specifier()?;
            let names = self.parse_struct_field_names()?;
            let semicolon = self.consume_kind(
                TokenKind::Semicolon,
                CompilerErrorCode::NoSemicolonAfterExpression,
                "expected ; after struct field declaration",
            )?;
            fields.push(StructFieldDecl {
                span: join_spans(field_type.span, semicolon.span),
                ty: field_type,
                names,
            });
        }

        self.consume_kind(
            TokenKind::RightBrace,
            CompilerErrorCode::BadTypeSpecifier,
            "expected } after struct body",
        )?;
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterStructure,
            "expected ; after struct declaration",
        )?;

        Ok(StructDecl {
            span: join_spans(ty.span, semicolon.span),
            name,
            fields,
        })
    }

    pub(super) fn parse_struct_field_names(&mut self) -> Result<Vec<NamedItem>, ParserError> {
        let mut names = Vec::new();
        loop {
            let name = self.consume_identifier(
                CompilerErrorCode::ParsingVariableList,
                "expected field name",
            )?;
            names.push(NamedItem {
                span: name.span,
                name: name.text,
            });
            if !self.matches_kind(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        Ok(names)
    }

    pub(super) fn parse_function_declaration_or_definition(
        &mut self,
        return_type: TypeSpec,
        name: Token,
    ) -> Result<FunctionDecl, ParserError> {
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::FunctionDefinitionMissingParameterList,
            "expected ( after function name",
        )?;
        let parameters = self.parse_parameter_list()?;
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::MalformedParameterList,
            "expected ) after parameter list",
        )?;

        if self.matches_kind(&TokenKind::Semicolon) {
            let semicolon = self.advance_required(
                CompilerErrorCode::NoSemicolonAfterExpression,
                "expected ; after parameter list",
            )?;
            return Ok(FunctionDecl {
                span: join_spans(return_type.span, semicolon.span),
                return_type,
                name: name.text,
                parameters,
                body: None,
            });
        }

        if !self.matches_kind(&TokenKind::LeftBrace) {
            return Err(self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                "expected ; or function body after parameter list",
            ));
        }

        let body = self.parse_block_statement()?;
        Ok(FunctionDecl {
            span: join_spans(return_type.span, body.span),
            return_type,
            name: name.text,
            parameters,
            body: Some(body),
        })
    }

    pub(super) fn parse_parameter_list(&mut self) -> Result<Vec<Parameter>, ParserError> {
        let mut parameters = Vec::new();
        if self.matches_kind(&TokenKind::RightParen) {
            return Ok(parameters);
        }

        loop {
            let ty = self.parse_non_void_type_specifier()?;
            let name = self.consume_identifier(
                CompilerErrorCode::BadVariableName,
                "expected parameter name",
            )?;
            let default = if self.matches_kind(&TokenKind::Assign) {
                self.advance();
                Some(self.parse_parameter_default_value()?)
            } else {
                None
            };
            let end_span = default.as_ref().map_or(name.span, |expr| expr.span);
            parameters.push(Parameter {
                span: join_spans(ty.span, end_span),
                ty,
                name: name.text,
                default,
            });

            if !self.matches_kind(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }

        Ok(parameters)
    }

    pub(super) fn parse_parameter_default_value(&mut self) -> Result<Expr, ParserError> {
        self.parse_expression()
    }

    pub(super) fn parse_any_type_specifier(&mut self) -> Result<TypeSpec, ParserError> {
        if self.matches_keyword(Keyword::Void) {
            let token = self.advance_required(
                CompilerErrorCode::InvalidDeclarationType,
                "expected void token",
            )?;
            return Ok(TypeSpec {
                span:     token.span,
                is_const: false,
                kind:     TypeKind::Void,
            });
        }
        self.parse_non_void_type_specifier()
    }

    pub(super) fn parse_non_void_type_specifier(&mut self) -> Result<TypeSpec, ParserError> {
        let const_token = if self.matches_keyword(Keyword::Const) {
            self.advance()
        } else {
            None
        };
        let is_const = const_token.is_some();

        let token = self.peek().cloned().ok_or_else(|| {
            self.error_here(
                CompilerErrorCode::InvalidDeclarationType,
                "unexpected EOF in type specifier",
            )
        })?;

        let kind = match token.kind {
            TokenKind::Keyword(Keyword::Int) => {
                self.advance();
                TypeKind::Int
            }
            TokenKind::Keyword(Keyword::Float) => {
                self.advance();
                TypeKind::Float
            }
            TokenKind::Keyword(Keyword::String) => {
                self.advance();
                TypeKind::String
            }
            TokenKind::Keyword(Keyword::Object) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is only valid for int, float, and string declarations",
                    ));
                }
                self.advance();
                TypeKind::Object
            }
            TokenKind::Keyword(Keyword::Vector) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is only valid for int, float, and string declarations",
                    ));
                }
                self.advance();
                TypeKind::Vector
            }
            TokenKind::Keyword(Keyword::Struct) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is not valid on struct declarations",
                    ));
                }
                self.advance();
                let name = self.consume_identifier(
                    CompilerErrorCode::InvalidDeclarationType,
                    "expected struct name after struct",
                )?;
                return Ok(TypeSpec {
                    span: join_spans(
                        const_token.map_or(token.span, |token| token.span),
                        name.span,
                    ),
                    is_const,
                    kind: TypeKind::Struct(name.text),
                });
            }
            TokenKind::Identifier if self.is_engine_structure_name(&token) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is not valid on engine structure declarations",
                    ));
                }
                self.advance();
                TypeKind::EngineStructure(token.text)
            }
            TokenKind::Identifier => {
                self.advance();
                TypeKind::Named(token.text)
            }
            _ => {
                return Err(self.error_here(
                    if is_const {
                        CompilerErrorCode::InvalidTypeForConstKeyword
                    } else {
                        CompilerErrorCode::InvalidDeclarationType
                    },
                    "expected a non-void type specifier",
                ));
            }
        };

        let start_span = const_token.map_or(token.span, |token| token.span);
        Ok(TypeSpec {
            span: join_spans(start_span, token.span),
            is_const,
            kind,
        })
    }

    pub(super) fn parse_declaration_after_name(
        &mut self,
        ty: TypeSpec,
        first_name: Token,
    ) -> Result<Declaration, ParserError> {
        let mut declarators = vec![self.parse_declarator_after_name(first_name)?];
        while self.matches_kind(&TokenKind::Comma) {
            self.advance();
            let name = self.consume_identifier(
                CompilerErrorCode::ParsingVariableList,
                "expected variable name after comma",
            )?;
            declarators.push(self.parse_declarator_after_name(name)?);
        }
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterExpression,
            "expected ; after declaration",
        )?;
        Ok(Declaration {
            span: join_spans(ty.span, semicolon.span),
            ty,
            declarators,
        })
    }

    pub(super) fn parse_declarator_after_name(
        &mut self,
        name: Token,
    ) -> Result<VarDeclarator, ParserError> {
        let initializer = if self.matches_kind(&TokenKind::Assign) {
            self.advance();
            Some(self.parse_expression()?)
        } else {
            None
        };
        let end_span = initializer.as_ref().map_or(name.span, |expr| expr.span);
        Ok(VarDeclarator {
            span: join_spans(name.span, end_span),
            name: name.text,
            initializer,
        })
    }
}
