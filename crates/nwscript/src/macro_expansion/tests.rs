use super::{
    IdentityMacro, MacroExpansionOptions, MacroPath, MacroRegistry, NwDelimiter, NwTokenStream,
    QUOTE_CONCAT_FUNCTION, QUOTE_REPEAT_FUNCTION, QUOTE_STATIC_FUNCTION, QuoteBindings,
    expand_bang_macros, expand_source_macros, expand_source_macros_traced, quote_nwscript,
    register_compiler_macros, register_nwscript_macro, render_nwscript_tokens,
    render_nwscript_tokens_bytes,
};
use crate::{
    ScriptString, SourceFile, SourceId, Span, Token, TokenKind, lex_bytes, lex_text, parse_tokens,
};

fn lex(input: &str) -> Vec<crate::Token> {
    match lex_text(SourceId::new(1), input) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("fixture should lex: {error}"),
    }
}

fn stream(input: &str) -> NwTokenStream {
    match NwTokenStream::from_tokens(&lex(input)) {
        Ok(stream) => stream,
        Err(error) => unreachable!("fixture should balance: {error}"),
    }
}

#[test]
fn balances_and_flattens_nested_groups() {
    let tokens = lex("call(alpha[1], { beta(); });");
    let stream = match NwTokenStream::from_tokens(&tokens) {
        Ok(stream) => stream,
        Err(error) => unreachable!("fixture should balance: {error}"),
    };
    let flattened = stream.into_tokens();
    let expected = tokens
        .into_iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .collect::<Vec<_>>();
    assert_eq!(flattened, expected);
}

#[test]
fn rejects_mismatched_delimiters() {
    let error = NwTokenStream::from_tokens(&lex("call([1));"));
    assert!(error.is_err());
}

#[test]
fn expands_namespaced_bang_macros_recursively() {
    let mut registry = MacroRegistry::new();
    assert!(
        registry
            .register_path("nwnrs::identity", IdentityMacro)
            .is_ok()
    );
    let expanded = match expand_bang_macros(
        lex("void main() { nwnrs::identity!(nwnrs::identity!(DoThing())); }"),
        &registry,
        MacroExpansionOptions::default(),
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("fixture should expand: {error}"),
    };
    assert!(expanded.iter().any(|token| token.text == "DoThing"));
    assert!(!expanded.iter().any(|token| token.text == "identity"));
}

#[test]
fn preserves_all_three_invocation_delimiters() {
    let mut registry = MacroRegistry::new();
    registry.register(
        match MacroPath::parse("identity") {
            Ok(path) => path,
            Err(error) => unreachable!("path should parse: {error}"),
        },
        IdentityMacro,
    );
    for (source, delimiter) in [
        ("identity!(1)", NwDelimiter::Parenthesis),
        ("identity![1]", NwDelimiter::Bracket),
        ("identity!{1}", NwDelimiter::Brace),
    ] {
        let trees = match NwTokenStream::from_tokens(&lex(source)) {
            Ok(stream) => stream,
            Err(error) => unreachable!("fixture should balance: {error}"),
        };
        let parsed = super::parse_macro_invocation(trees.trees(), 0);
        match parsed {
            Ok(Some((invocation, _))) => assert_eq!(invocation.delimiter, delimiter),
            Ok(None) => unreachable!("fixture should be a macro invocation"),
            Err(error) => unreachable!("fixture should parse: {error}"),
        }
    }
}

#[test]
fn reports_unknown_macro_with_its_path() {
    let error = expand_bang_macros(
        lex("void main() { missing!(); }"),
        &MacroRegistry::new(),
        MacroExpansionOptions::default(),
    );
    match error {
        Ok(_) => unreachable!("unknown macro should fail"),
        Err(error) => assert!(error.to_string().contains("unknown macro `missing`")),
    }
}

#[test]
fn quote_interpolates_single_and_repeated_bindings() {
    let mut bindings = QuoteBindings::new();
    bindings.insert("name", stream("Generated"));
    bindings.insert_repeated(
        "arguments",
        vec![stream("first"), stream("second"), stream("third")],
    );
    let quoted = match quote_nwscript(
        &stream("void $name($($arguments),*) { $$ignored; }"),
        &bindings,
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("fixture should quote: {error}"),
    };
    let texts = quoted
        .into_tokens()
        .into_iter()
        .map(|token| token.text)
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        vec![
            "void",
            "Generated",
            "(",
            "first",
            ",",
            "second",
            ",",
            "third",
            ")",
            "{",
            "$",
            "ignored",
            ";",
            "}"
        ]
    );
}

#[test]
fn quote_rejects_mismatched_zipped_repetition_lengths() {
    let mut bindings = QuoteBindings::new();
    bindings.insert_repeated("left", vec![stream("A"), stream("B")]);
    bindings.insert_repeated("right", vec![stream("1")]);
    let error = quote_nwscript(&stream("$($left = $right;)*"), &bindings)
        .expect_err("zipped bindings with different lengths should fail");
    assert!(error.to_string().contains("different lengths"));
}

#[test]
fn source_defined_macro_rules_expand_into_parseable_nwscript() {
    let source = r#"
        macro_rules! make_handler {
            ($name:ident, $body:tokens) => {
                void $name() { $body }
            };
        }

        make_handler!(main, int value = 7;)
    "#;
    let expanded = match expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("fixture should expand: {error}"),
    };
    let script = match parse_tokens(expanded, None) {
        Ok(script) => script,
        Err(error) => unreachable!("expanded fixture should parse: {error}"),
    };
    assert_eq!(script.items.len(), 1);
}

#[test]
fn declarative_macro_static_tokens_use_the_invocation_span() {
    let source = "macro_rules! make { ($name:ident) => { void $name() {} }; }\nmake!(Generated)";
    let original = lex(source);
    let invocation = original
        .iter()
        .rev()
        .find(|token| token.text == "make")
        .expect("invocation token")
        .span;
    let captured = original
        .iter()
        .find(|token| token.text == "Generated")
        .expect("captured token")
        .span;
    let expanded = expand_source_macros(
        original,
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("expand declarative macro");

    let static_span = expanded
        .iter()
        .find(|token| token.text == "void")
        .expect("static output token")
        .span;
    assert_eq!(static_span.source_id, invocation.source_id);
    assert_eq!(static_span.start, invocation.start);
    assert!(static_span.end > invocation.end);
    assert_eq!(
        expanded
            .iter()
            .find(|token| token.text == "Generated")
            .expect("captured output token")
            .span,
        captured
    );
}

#[test]
fn source_defined_macros_can_expand_other_macros() {
    let source = r#"
        macro_rules! inner {
            ($value:literal) => { $value };
        }
        macro_rules! outer {
            ($value:literal) => { inner!($value) };
        }
        void main() { int value = outer!(11); }
    "#;
    let expanded = match expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("fixture should expand recursively: {error}"),
    };
    assert!(expanded.iter().any(|token| token.text == "11"));
    assert!(!expanded.iter().any(|token| token.text == "inner"));
    assert!(!expanded.iter().any(|token| token.text == "outer"));
}

#[test]
fn traced_expansion_records_immediate_nested_outputs() {
    let source = r#"
        macro_rules! inner { () => { 7 }; }
        macro_rules! outer { () => { inner!() }; }
        void main() { int value = outer!(); }
    "#;
    let (_expanded, trace) = expand_source_macros_traced(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("traced macros should expand");
    let [outer, inner] = trace.as_slice() else {
        panic!("expected exactly two trace records");
    };
    assert_eq!(outer.path.to_string(), "outer");
    assert_eq!(outer.depth, 1);
    assert!(render_nwscript_tokens(&outer.output).contains("inner"));
    assert_eq!(inner.path.to_string(), "inner");
    assert_eq!(inner.depth, 2);
}

#[test]
fn declarative_matcher_repetition_supports_separators_and_quantifiers() {
    let source = r#"
        macro_rules! call_all {
            ($($name:ident),+) => {
                void main() { $($name();)* }
            };
        }
        call_all!(First, Second, Third)
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("repeated declarative matcher should expand");
    let names = expanded
        .iter()
        .filter(|token| matches!(token.text.as_str(), "First" | "Second" | "Third"))
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["First", "Second", "Third"]);
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn declarative_matcher_repetition_handles_empty_optional_and_required_inputs() {
    let source = r#"
        macro_rules! optional_constant {
            ($($value:literal)?) => {
                $(const int OPTIONAL = $value;)?
                void main() {}
            };
        }
        optional_constant!()
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("empty optional repetition should expand");
    assert!(!expanded.iter().any(|token| token.text == "OPTIONAL"));
    assert!(parse_tokens(expanded, None).is_ok());

    let present = source.replace("optional_constant!()", "optional_constant!(9)");
    let expanded = expand_source_macros(
        lex(&present),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("present optional repetition should expand");
    assert!(expanded.iter().any(|token| token.text == "OPTIONAL"));
    assert!(parse_tokens(expanded, None).is_ok());

    let required = r#"
        macro_rules! at_least_one {
            ($($value:literal),+) => { void main() {} };
        }
        at_least_one!()
    "#;
    assert!(
        expand_source_macros(
            lex(required),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .is_err()
    );
}

#[test]
fn declarative_repetition_supports_nested_and_zipped_bindings() {
    let source = r#"
        macro_rules! constants {
            ($(($($name:ident = $value:literal),*));*) => {
                $($(const int $name = $value;)*)*
                void main() {}
            };
        }
        constants!((FIRST = 1, SECOND = 2); (THIRD = 3))
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("nested declarative matcher should expand");
    for expected in ["FIRST", "1", "SECOND", "2", "THIRD", "3"] {
        assert!(expanded.iter().any(|token| token.text == expected));
    }
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn compiler_quote_lowers_interpolation_to_tokenstream_calls() {
    let mut registry = MacroRegistry::new();
    assert!(register_compiler_macros(&mut registry).is_ok());
    let expanded = match expand_bang_macros(
        lex(
            "tokenstream Build(tokenstream name, tokenstream body) { return quote!{ void $name() \
             { $body } }; }",
        ),
        &registry,
        MacroExpansionOptions::default(),
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("fixture should lower quote: {error}"),
    };
    assert!(
        expanded
            .iter()
            .any(|token| token.text == QUOTE_STATIC_FUNCTION)
    );
    assert!(
        expanded
            .iter()
            .any(|token| token.text == QUOTE_CONCAT_FUNCTION)
    );
    assert!(!expanded.iter().any(|token| token.text == "quote"));
    assert!(!expanded.iter().any(|token| token.kind == TokenKind::Dollar));
}

#[test]
fn renderer_round_trips_strings_and_extended_attributes() {
    let original = stream(
        r#"#[nwnrs::event::object_use::before(after(ValidateUse))] void handler(string value = "a\n\"\\");"#,
    );
    let rendered = render_nwscript_tokens(&original);
    let reparsed = stream(&rendered);
    assert_eq!(
        original
            .into_tokens()
            .into_iter()
            .map(|token| (token.kind, token.text))
            .collect::<Vec<_>>(),
        reparsed
            .into_tokens()
            .into_iter()
            .map(|token| (token.kind, token.text))
            .collect::<Vec<_>>()
    );
}

#[test]
fn byte_renderer_round_trips_every_string_payload_byte_without_utf8() {
    let payload = (0_u8..=u8::MAX).collect::<Vec<_>>();
    let text = payload.iter().copied().map(char::from).collect::<String>();
    let stream = NwTokenStream::from_trees(vec![super::NwTokenTree::Token(Token::new(
        TokenKind::String,
        Span::new(SourceId::new(1), 0, payload.len()),
        text,
    ))]);
    let rendered = render_nwscript_tokens_bytes(&stream);
    assert!(rendered.is_ascii());
    let tokens = lex_bytes(SourceId::new(2), &rendered).expect("rendered bytes should lex");
    let string = tokens
        .iter()
        .find(|token| token.kind == TokenKind::String)
        .expect("rendered string token");
    assert_eq!(
        ScriptString::from_lexed_text(&string.text).as_bytes(),
        payload
    );
}

#[test]
fn compiler_quote_lowers_repetition_to_collection_runtime() {
    let mut registry = MacroRegistry::new();
    assert!(register_compiler_macros(&mut registry).is_ok());
    let expanded = expand_bang_macros(
        lex("tokenstream Build(tokenstream_list values) { return quote!($($values),*); }"),
        &registry,
        MacroExpansionOptions::default(),
    )
    .expect("compiler quote repetition should lower");
    assert!(
        expanded
            .iter()
            .any(|token| token.text == QUOTE_REPEAT_FUNCTION)
    );
    assert!(!expanded.iter().any(|token| token.kind == TokenKind::Dollar));
}

#[test]
fn nwscript_procedural_quote_repeats_and_zips_tokenstream_lists() {
    let source = r#"
        proc_macro! make_constants {
            tokenstream make_constants(tokenstream input) {
                tokenstream_list names = __NWNRS_TokenStreamListNew();
                tokenstream_list values = __NWNRS_TokenStreamListNew();
                names = __NWNRS_TokenStreamListPush(
                    names, __NWNRS_TokenStreamGet(input, 0));
                names = __NWNRS_TokenStreamListPush(
                    names, __NWNRS_TokenStreamGet(input, 2));
                values = __NWNRS_TokenStreamListPush(
                    values, __NWNRS_TokenStreamGet(input, 1));
                values = __NWNRS_TokenStreamListPush(
                    values, __NWNRS_TokenStreamGet(input, 3));
                return quote! { $(const int $names = $values;)* };
            }
        }

        make_constants!(FIRST 1 SECOND 2)
        void main() {}
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("procedural quote repetition should execute");
    for expected in ["FIRST", "1", "SECOND", "2"] {
        assert!(expanded.iter().any(|token| token.text == expected));
    }
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn procedural_quote_repetition_preserves_high_byte_string_tokens() {
    let source = r#"
        proc_macro! repeat_value {
            tokenstream repeat_value(tokenstream input) {
                tokenstream_list values = __NWNRS_TokenStreamListNew();
                values = __NWNRS_TokenStreamListPush(
                    values, __NWNRS_TokenStreamGet(input, 0));
                return quote! { string Preserved = $($values)*; };
            }
        }

        repeat_value!("\x80")
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("quote repetition must preserve exact token bytes");
    let value = expanded
        .iter()
        .find(|token| token.kind == TokenKind::String)
        .expect("expanded string token");
    assert_eq!(
        ScriptString::from_lexed_text(&value.text).as_bytes(),
        &[0x80]
    );
}

#[test]
fn procedural_macro_arena_rejects_aggregate_handle_exhaustion() {
    let source = r#"
        proc_macro! identity {
            tokenstream identity(tokenstream input) {
                return input;
            }
        }
        identity!(value)
    "#;
    let error = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions {
            arena_handle_limit: 3,
            ..MacroExpansionOptions::default()
        },
    )
    .expect_err("the arena must reject its fourth retained handle");

    assert!(error.message.contains("arena handle budget exceeded"));
}

#[test]
fn nwscript_procedural_quote_supports_nested_tokenstream_lists() {
    let source = r#"
        proc_macro! make_constants {
            tokenstream make_constants(tokenstream input) {
                tokenstream_list first = __NWNRS_TokenStreamListNew();
                first = __NWNRS_TokenStreamListPush(
                    first, __NWNRS_TokenStreamGet(input, 0));
                first = __NWNRS_TokenStreamListPush(
                    first, __NWNRS_TokenStreamGet(input, 1));

                tokenstream_list second = __NWNRS_TokenStreamListNew();
                second = __NWNRS_TokenStreamListPush(
                    second, __NWNRS_TokenStreamGet(input, 2));

                tokenstream_list rows = __NWNRS_TokenStreamListNew();
                rows = __NWNRS_TokenStreamListPushList(rows, first);
                rows = __NWNRS_TokenStreamListPushList(rows, second);
                return quote! { $($(const int $rows = 1;)*)* void main() {} };
            }
        }

        make_constants!(FIRST SECOND THIRD)
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("nested procedural quote repetition should execute");
    for expected in ["FIRST", "SECOND", "THIRD"] {
        assert!(expanded.iter().any(|token| token.text == expected));
    }
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn nwscript_procedural_macro_can_parse_with_a_token_cursor() {
    let source = r#"
        proc_macro! rebuild_function {
            tokenstream rebuild_function(tokenstream input) {
                token_cursor cursor = __NWNRS_TokenCursorNew(input);
                tokenstream function = __NWNRS_TokenCursorParseFunction(cursor);
                if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                    __NWNRS_MacroErrorAt(
                        __NWNRS_TokenCursorRemaining(cursor),
                        "expected exactly one function");
                }
                return quote! { $function };
            }
        }

        rebuild_function!(void Generated(int value) { int copy = value; })
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("token cursor should parse a function");
    assert!(expanded.iter().any(|token| token.text == "Generated"));
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn nwscript_token_cursor_parses_separated_expressions_for_quote_repetition() {
    let source = r#"
        proc_macro! make_body {
            tokenstream make_body(tokenstream input) {
                token_cursor cursor = __NWNRS_TokenCursorNew(input);
                tokenstream_list expressions = __NWNRS_TokenCursorParseSeparated(
                    cursor, "expr", ",", 1, 0);
                if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                    __NWNRS_MacroErrorAt(
                        __NWNRS_TokenCursorRemaining(cursor),
                        "unexpected input");
                }
                return quote! { void main() { $($expressions;)* } };
            }
        }

        make_body!(First(), Second(2), 3 + 4)
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("token cursor should parse separated expressions");
    for expected in ["First", "Second", "3", "4"] {
        assert!(expanded.iter().any(|token| token.text == expected));
    }
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn nwscript_token_cursor_keeps_if_else_as_one_statement() {
    let source = r#"
        proc_macro! parse_statements {
            tokenstream parse_statements(tokenstream input) {
                token_cursor cursor = __NWNRS_TokenCursorNew(input);
                tokenstream first = __NWNRS_TokenCursorParseStatement(cursor);
                tokenstream second = __NWNRS_TokenCursorParseStatement(cursor);
                if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                    __NWNRS_MacroError("expected exactly two statements");
                }
                return quote! { void main() { $first $second } };
            }
        }

        parse_statements!(
            if (TRUE) { First(); } else if (FALSE) { Second(); } else { Third(); }
            int value = 7;
        )
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("token cursor should consume a complete if/else statement");
    for expected in ["First", "Second", "Third", "value"] {
        assert!(expanded.iter().any(|token| token.text == expected));
    }
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn nwscript_token_cursor_exposes_named_fragment_parsers() {
    let source = r#"
        proc_macro! parse_fragments {
            tokenstream parse_fragments(tokenstream input) {
                token_cursor cursor = __NWNRS_TokenCursorNew(input);
                tokenstream name = __NWNRS_TokenCursorParseIdentifier(cursor);
                __NWNRS_TokenCursorExpect(cursor, ",");
                tokenstream value = __NWNRS_TokenCursorParseLiteral(cursor);
                __NWNRS_TokenCursorExpect(cursor, ",");
                tokenstream path = __NWNRS_TokenCursorParsePath(cursor);
                __NWNRS_TokenCursorExpect(cursor, ",");
                tokenstream type = __NWNRS_TokenCursorParseType(cursor);
                __NWNRS_TokenCursorExpect(cursor, ",");
                tokenstream tree = __NWNRS_TokenCursorParseTree(cursor);
                __NWNRS_TokenCursorExpect(cursor, ",");
                tokenstream structure = __NWNRS_TokenCursorParseStruct(cursor);
                if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                    __NWNRS_MacroError("unexpected fragment input");
                }
                return quote! {
                    $structure
                    const int $name = $value;
                    void main() {}
                };
            }
        }

        parse_fragments!(
            Generated, 17, project::value, struct Example, (one + two),
            struct GeneratedStruct { int member; };
        )
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("named cursor fragment parsers should execute");
    for expected in ["Generated", "17", "GeneratedStruct", "member"] {
        assert!(expanded.iter().any(|token| token.text == expected));
    }
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn nwscript_project_macro_can_open_groups_and_sort_collections() {
    let source = r#"
        proc_macro! sorted_calls {
            tokenstream sorted_calls(tokenstream input) {
                token_cursor outer = __NWNRS_TokenCursorNew(input);
                tokenstream group = __NWNRS_TokenCursorParseTree(outer);
                token_cursor inner = __NWNRS_TokenCursorNew(
                    __NWNRS_TokenGroupContents(group));
                tokenstream_list names = __NWNRS_TokenCursorParseSeparated(
                    inner, "ident", ",", 1, 0);
                names = __NWNRS_TokenStreamListSort(names);
                return quote! { void main() { $($names();)* } };
            }
        }
        sorted_calls!({Zulu, Alpha, Middle})
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("project token utilities should execute");
    let rendered = render_nwscript_tokens(
        &NwTokenStream::from_tokens(&expanded).expect("expanded stream should balance"),
    );
    let alpha = rendered.find("Alpha").expect("Alpha call");
    let middle = rendered.find("Middle").expect("Middle call");
    let zulu = rendered.find("Zulu").expect("Zulu call");
    assert!(alpha < middle && middle < zulu);
    assert!(parse_tokens(expanded, None).is_ok());
}

#[test]
fn nwscript_macro_error_at_reports_the_input_location() {
    let source = r#"
        proc_macro! fail_at_input {
            tokenstream fail_at_input(tokenstream input) {
                __NWNRS_MacroErrorAt(input, "deliberate failure");
                return input;
            }
        }
        fail_at_input!(Problem)
    "#;
    let error = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect_err("macro should report a located error");
    assert!(error.to_string().contains("deliberate failure"));
    let file = SourceFile::new(SourceId::new(1), "macro-input.nss", source);
    assert_eq!(
        file.span_text(
            error
                .span
                .expect("macro error should retain its input span")
        ),
        Some("Problem".to_string())
    );
}

#[test]
fn nwscript_procedural_macro_compiles_executes_and_returns_tokens() {
    let implementation = r#"
        tokenstream make_function(tokenstream input) {
            return quote! {
                void generated() { $input }
            };
        }
    "#;
    let mut registry = MacroRegistry::new();
    assert!(
        register_nwscript_macro(
            &mut registry,
            "make_function",
            "make_function.nss",
            "make_function",
            implementation,
        )
        .is_ok()
    );
    let expanded = match expand_bang_macros(
        lex("make_function!(int value = 7;)"),
        &registry,
        MacroExpansionOptions::default(),
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("procedural macro should execute: {error}"),
    };
    let script = match parse_tokens(expanded, None) {
        Ok(script) => script,
        Err(error) => unreachable!("generated function should parse: {error}"),
    };
    assert_eq!(script.items.len(), 1);
}

#[test]
fn source_defined_nwscript_procedural_macro_is_collected_and_removed() {
    let source = r#"
        proc_macro! project::make_constant {
            tokenstream make_constant(tokenstream input) {
                return quote! { const int GENERATED = $input; };
            }
        }

        project::make_constant!(19)
        void main() { int value = GENERATED; }
    "#;
    let expanded = match expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("source procedural macro should expand: {error}"),
    };
    assert!(!expanded.iter().any(|token| token.text == "proc_macro"));
    assert!(!expanded.iter().any(|token| token.text == "make_constant"));
    let script = match parse_tokens(expanded, None) {
        Ok(script) => script,
        Err(error) => unreachable!("procedural output should parse: {error}"),
    };
    assert_eq!(script.items.len(), 2);
}

#[test]
fn procedural_macro_static_tokens_use_the_invocation_span() {
    let source = r#"
        proc_macro! make_constant {
            tokenstream make_constant(tokenstream input) {
                return quote! { const int GENERATED = $input; };
            }
        }
        make_constant!(19)
    "#;
    let original = lex(source);
    let invocation = original
        .iter()
        .rev()
        .find(|token| token.text == "make_constant")
        .expect("invocation token")
        .span;
    let captured = original
        .iter()
        .find(|token| token.text == "19")
        .expect("captured token")
        .span;
    let expanded = expand_source_macros(
        original,
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("expand procedural macro");

    let static_span = expanded
        .iter()
        .find(|token| token.text == "const")
        .expect("static output token")
        .span;
    assert_eq!(static_span.source_id, invocation.source_id);
    assert_eq!(static_span.start, invocation.start);
    assert!(static_span.end > invocation.end);
    assert_eq!(
        expanded
            .iter()
            .find(|token| token.text == "19")
            .expect("captured output token")
            .span,
        captured
    );
}

#[test]
fn nwscript_procedural_macro_can_inspect_input_token_trees() {
    let source = r#"
        proc_macro! make_named_function {
            tokenstream make_named_function(tokenstream input) {
                if (__NWNRS_TokenStreamLength(input) != 1) {
                    __NWNRS_MacroError("expected one function name");
                }
                tokenstream name = __NWNRS_TokenStreamGet(input, 0);
                if (__NWNRS_TokenKind(name) != "identifier") {
                    __NWNRS_MacroError("function name must be an identifier");
                }
                return quote! { void $name() {} };
            }
        }

        make_named_function!(Generated)
    "#;
    let expanded = match expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    ) {
        Ok(tokens) => tokens,
        Err(error) => unreachable!("inspection macro should expand: {error}"),
    };
    let script = match parse_tokens(expanded, None) {
        Ok(script) => script,
        Err(error) => unreachable!("inspected output should parse: {error}"),
    };
    assert_eq!(script.items.len(), 1);
}

#[test]
fn procedural_macro_token_text_preserves_high_bytes() {
    let source = r#"
        proc_macro! require_high_byte {
            tokenstream require_high_byte(tokenstream input) {
                tokenstream value = __NWNRS_TokenStreamGet(input, 0);
                if (__NWNRS_TokenText(value) != "\x80") {
                    __NWNRS_MacroError("token text bytes changed");
                }
                return quote! { int BytePreserved = 1; };
            }
        }

        require_high_byte!("\x80")
    "#;
    let expanded = expand_source_macros(
        lex(source),
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .expect("procedural macro must observe the exact string byte");
    assert!(expanded.iter().any(|token| token.text == "BytePreserved"));
}
