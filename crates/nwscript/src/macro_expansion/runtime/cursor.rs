use super::*;

#[derive(Debug, Clone, Copy)]
enum CursorFragmentKind {
    Identifier,
    Literal,
    Tree,
    Path,
    Expression,
    Type,
    Statement,
    Function,
    Struct,
}

impl CursorFragmentKind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ident" | "identifier" => Self::Identifier,
            "literal" => Self::Literal,
            "tt" | "tree" => Self::Tree,
            "path" => Self::Path,
            "expr" | "expression" => Self::Expression,
            "type" => Self::Type,
            "stmt" | "statement" => Self::Statement,
            "fn" | "function" => Self::Function,
            "struct" => Self::Struct,
            _ => return None,
        })
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::Literal => "literal",
            Self::Tree => "token tree",
            Self::Path => "path",
            Self::Expression => "expression",
            Self::Type => "type",
            Self::Statement => "statement",
            Self::Function => "function",
            Self::Struct => "struct",
        }
    }
}

pub(super) fn define_token_cursor_commands(
    vm: &mut crate::Vm,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(20, move |script| {
            let stream = pop_macro_tokenstream_handle(script)?;
            let stream = arena.borrow().balanced(stream).map_err(macro_vm_error)?;
            let handle = arena.borrow_mut().insert_cursor(stream)?;
            push_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(21, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let position = arena
                .borrow()
                .cursor(cursor)
                .ok_or_else(|| unknown_cursor_handle(cursor))?
                .position;
            script.push_int(
                i32::try_from(position).map_err(|error| crate::VmError::Setup {
                    message: format!(
                        "token-cursor position exceeds NWScript integer range: {error}"
                    ),
                })?,
            );
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(22, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let arena = arena.borrow();
            let cursor = arena
                .cursor(cursor)
                .ok_or_else(|| unknown_cursor_handle(cursor))?;
            script.push_int(i32::from(cursor.position >= cursor.stream.len()));
            Ok(())
        });
    }
    define_cursor_tree_command(vm, 23, Rc::clone(&arena), false);
    define_cursor_tree_command(vm, 24, Rc::clone(&arena), true);
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(25, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let expected = pop_macro_token_text(script)?;
            let mut arena = arena.borrow_mut();
            let cursor = arena
                .cursor_mut(cursor)
                .ok_or_else(|| unknown_cursor_handle(cursor))?;
            let matched = cursor
                .stream
                .trees()
                .get(cursor.position)
                .is_some_and(|tree| macro_tree_text(tree) == expected);
            if matched {
                cursor.position = cursor.position.saturating_add(1);
            }
            script.push_int(i32::from(matched));
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(26, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let expected = pop_macro_token_text(script)?;
            let tree = {
                let mut arena = arena.borrow_mut();
                let cursor = arena
                    .cursor_mut(cursor)
                    .ok_or_else(|| unknown_cursor_handle(cursor))?;
                let tree = cursor
                    .stream
                    .trees()
                    .get(cursor.position)
                    .cloned()
                    .ok_or_else(|| crate::VmError::Setup {
                        message: format!("expected {expected:?}, reached end of macro input"),
                    })?;
                if macro_tree_text(&tree) != expected {
                    return Err(crate::VmError::Setup {
                        message: format!(
                            "expected {expected:?}, found {:?}",
                            macro_tree_text(&tree)
                        ),
                    });
                }
                cursor.position = cursor.position.saturating_add(1);
                tree
            };
            let handle = arena
                .borrow_mut()
                .insert(NwTokenStream::from_trees(vec![tree]))?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    for (command, kind) in [
        (27, CursorFragmentKind::Identifier),
        (28, CursorFragmentKind::Literal),
        (29, CursorFragmentKind::Tree),
        (30, CursorFragmentKind::Path),
        (31, CursorFragmentKind::Expression),
        (32, CursorFragmentKind::Type),
        (33, CursorFragmentKind::Statement),
        (34, CursorFragmentKind::Function),
        (35, CursorFragmentKind::Struct),
    ] {
        define_cursor_parse_command(vm, command, Rc::clone(&arena), source_id, kind);
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(36, move |script| {
            let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let kind_text = pop_macro_ascii(script, "token-cursor fragment kind")?;
            let kind =
                CursorFragmentKind::parse(&kind_text).ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown token-cursor fragment kind {kind_text:?}"),
                })?;
            let separator = pop_macro_token_text(script)?;
            let minimum = macro_index(script.pop_int()?, script)?;
            let maximum = macro_index(script.pop_int()?, script)?;
            let mut parsed = Vec::new();
            loop {
                if maximum != 0 && parsed.len() >= maximum {
                    break;
                }
                let checkpoint = {
                    let arena = arena.borrow();
                    arena
                        .cursor(cursor_handle)
                        .ok_or_else(|| unknown_cursor_handle(cursor_handle))?
                        .position
                };
                let value = {
                    let mut arena = arena.borrow_mut();
                    parse_cursor_fragment(
                        arena
                            .cursor_mut(cursor_handle)
                            .ok_or_else(|| unknown_cursor_handle(cursor_handle))?,
                        kind,
                        source_id,
                    )
                };
                let Ok(value) = value else {
                    if let Some(cursor) = arena.borrow_mut().cursor_mut(cursor_handle) {
                        cursor.position = checkpoint;
                    }
                    break;
                };
                let stream = arena.borrow_mut().insert(value)?;
                parsed.push(TokenStreamListValue::Stream(stream));
                let has_separator = {
                    let mut arena = arena.borrow_mut();
                    let cursor = arena
                        .cursor_mut(cursor_handle)
                        .ok_or_else(|| unknown_cursor_handle(cursor_handle))?;
                    let matched = cursor
                        .stream
                        .trees()
                        .get(cursor.position)
                        .is_some_and(|tree| macro_tree_text(tree) == separator);
                    if matched {
                        cursor.position = cursor.position.saturating_add(1);
                    }
                    matched
                };
                if !has_separator {
                    break;
                }
            }
            if parsed.len() < minimum {
                return Err(crate::VmError::Setup {
                    message: format!(
                        "expected at least {minimum} {} value(s), parsed {}",
                        kind.name(),
                        parsed.len()
                    ),
                });
            }
            let handle = arena.borrow_mut().insert_list(parsed)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(37, move |script| {
            let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let remaining = {
                let arena = arena.borrow();
                let cursor = arena
                    .cursor(cursor_handle)
                    .ok_or_else(|| unknown_cursor_handle(cursor_handle))?;
                NwTokenStream::from_trees(
                    cursor
                        .stream
                        .trees()
                        .get(cursor.position..)
                        .unwrap_or_default()
                        .to_vec(),
                )
            };
            let handle = arena.borrow_mut().insert(remaining)?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(38, move |script| {
            let input = pop_macro_tokenstream_handle(script)?;
            let message = script.pop_string()?.display_text();
            let span = arena
                .borrow()
                .balanced(input)
                .map_err(|error| crate::VmError::Setup {
                    message: format!(
                        "procedural macro reported a diagnostic for an invalid tokenstream handle \
                         {input}: {error}"
                    ),
                })?
                .trees()
                .first()
                .map(NwTokenTree::span);
            match span {
                Some(span) => Err(crate::VmError::MacroDiagnostic {
                    span,
                    message,
                }),
                None => Err(crate::VmError::Setup {
                    message: format!(
                        "procedural macro reported for tokenstream {input}: {message}"
                    ),
                }),
            }
        });
    }
}

pub(super) fn define_token_project_commands(
    vm: &mut crate::Vm,
    arena: Rc<RefCell<TokenStreamArena>>,
) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(39, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let values = arena
                .borrow()
                .list(list)
                .map(<[TokenStreamListValue]>::to_vec)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {list}"),
                })?;
            let mut keyed = {
                let arena = arena.borrow();
                values
                    .into_iter()
                    .map(|value| {
                        let TokenStreamListValue::Stream(handle) = value else {
                            return Err(crate::VmError::Setup {
                                message: "cannot text-sort a nested tokenstream list".to_string(),
                            });
                        };
                        let key = arena
                            .balanced(handle)
                            .map(|stream| render_nwscript_tokens(&stream))
                            .map_err(macro_vm_error)?;
                        Ok((key, TokenStreamListValue::Stream(handle)))
                    })
                    .collect::<Result<Vec<_>, crate::VmError>>()?
            };
            keyed.sort_by(|left, right| left.0.cmp(&right.0));
            let values = keyed.into_iter().map(|(_key, value)| value).collect();
            let handle = arena.borrow_mut().insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    vm.define_simple_command(40, move |script| {
        let tree = pop_single_macro_tree(script, &arena)?;
        let NwTokenTree::Group(group) = tree else {
            return Err(crate::VmError::Setup {
                message: "token group contents require exactly one delimited group".to_string(),
            });
        };
        let handle = arena.borrow_mut().insert(group.stream)?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

fn define_cursor_tree_command(
    vm: &mut crate::Vm,
    command: u16,
    arena: Rc<RefCell<TokenStreamArena>>,
    advance: bool,
) {
    vm.define_simple_command(command, move |script| {
        let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
        let tree = {
            let mut arena = arena.borrow_mut();
            let cursor = arena
                .cursor_mut(cursor_handle)
                .ok_or_else(|| unknown_cursor_handle(cursor_handle))?;
            let tree = cursor
                .stream
                .trees()
                .get(cursor.position)
                .cloned()
                .ok_or_else(|| crate::VmError::Setup {
                    message: "token cursor reached end of input".to_string(),
                })?;
            if advance {
                cursor.position = cursor.position.saturating_add(1);
            }
            tree
        };
        let handle = arena
            .borrow_mut()
            .insert(NwTokenStream::from_trees(vec![tree]))?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

fn define_cursor_parse_command(
    vm: &mut crate::Vm,
    command: u16,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
    kind: CursorFragmentKind,
) {
    vm.define_simple_command(command, move |script| {
        let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
        let parsed = {
            let mut arena = arena.borrow_mut();
            parse_cursor_fragment(
                arena
                    .cursor_mut(cursor_handle)
                    .ok_or_else(|| unknown_cursor_handle(cursor_handle))?,
                kind,
                source_id,
            )
            .map_err(macro_vm_error)?
        };
        let handle = arena.borrow_mut().insert(parsed)?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

fn unknown_cursor_handle(handle: u32) -> crate::VmError {
    crate::VmError::Setup {
        message: format!("unknown compiler token-cursor handle {handle}"),
    }
}

fn macro_tree_text(tree: &NwTokenTree) -> String {
    match tree {
        NwTokenTree::Token(token) => token.text.clone(),
        NwTokenTree::Group(_) => {
            render_nwscript_tokens(&NwTokenStream::from_trees(vec![tree.clone()]))
        }
    }
}

fn parse_cursor_fragment(
    cursor: &mut TokenCursorState,
    kind: CursorFragmentKind,
    source_id: SourceId,
) -> Result<NwTokenStream, MacroExpansionError> {
    let trees = cursor.stream.trees();
    let start = cursor.position;
    let first = trees.get(start).ok_or_else(|| {
        MacroExpansionError::without_span(format!("expected {}, reached end of input", kind.name()))
    })?;
    let end = match kind {
        CursorFragmentKind::Tree => start.saturating_add(1),
        CursorFragmentKind::Identifier => match first {
            NwTokenTree::Token(token) if token.kind == TokenKind::Identifier => start + 1,
            _ => return Err(expected_cursor_fragment(first, kind)),
        },
        CursorFragmentKind::Literal => match first {
            NwTokenTree::Token(token) if literal_kind(&token.kind) => start + 1,
            _ => return Err(expected_cursor_fragment(first, kind)),
        },
        CursorFragmentKind::Path => cursor_path_end(trees, start)?,
        CursorFragmentKind::Type => cursor_type_end(trees, start)?,
        CursorFragmentKind::Expression => trees
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(index, tree)| match tree {
                NwTokenTree::Token(token)
                    if matches!(token.kind, TokenKind::Comma | TokenKind::Semicolon) =>
                {
                    Some(index)
                }
                _ => None,
            })
            .unwrap_or(trees.len()),
        CursorFragmentKind::Statement => cursor_statement_end(trees, start)?,
        CursorFragmentKind::Function => cursor_body_item_end(trees, start, false)?,
        CursorFragmentKind::Struct => cursor_body_item_end(trees, start, true)?,
    };
    if end <= start {
        return Err(expected_cursor_fragment(first, kind));
    }
    let stream = NwTokenStream::from_trees(
        trees
            .get(start..end)
            .ok_or_else(|| expected_cursor_fragment(first, kind))?
            .to_vec(),
    );
    validate_cursor_fragment(&stream, kind, source_id)?;
    cursor.position = end;
    Ok(stream)
}

fn expected_cursor_fragment(tree: &NwTokenTree, kind: CursorFragmentKind) -> MacroExpansionError {
    MacroExpansionError::new(
        tree.span(),
        format!(
            "expected {}, found {:?}",
            kind.name(),
            macro_tree_text(tree)
        ),
    )
}

fn cursor_path_end(trees: &[NwTokenTree], start: usize) -> Result<usize, MacroExpansionError> {
    let Some(NwTokenTree::Token(first)) = trees.get(start) else {
        return Err(MacroExpansionError::without_span(
            "expected identifier path",
        ));
    };
    if first.kind != TokenKind::Identifier {
        return Err(MacroExpansionError::new(
            first.span,
            "expected identifier path",
        ));
    }
    let mut end = start + 1;
    while let (
        Some(NwTokenTree::Token(left)),
        Some(NwTokenTree::Token(right)),
        Some(NwTokenTree::Token(segment)),
    ) = (trees.get(end), trees.get(end + 1), trees.get(end + 2))
    {
        if left.kind != TokenKind::Colon
            || right.kind != TokenKind::Colon
            || segment.kind != TokenKind::Identifier
        {
            break;
        }
        end += 3;
    }
    Ok(end)
}

fn cursor_type_end(trees: &[NwTokenTree], start: usize) -> Result<usize, MacroExpansionError> {
    let Some(NwTokenTree::Token(first)) = trees.get(start) else {
        return Err(MacroExpansionError::without_span("expected NWScript type"));
    };
    if first.text == "struct" {
        let Some(NwTokenTree::Token(name)) = trees.get(start + 1) else {
            return Err(MacroExpansionError::new(
                first.span,
                "struct type requires a name",
            ));
        };
        if name.kind != TokenKind::Identifier {
            return Err(MacroExpansionError::new(
                name.span,
                "struct type requires a name",
            ));
        }
        return Ok(start + 2);
    }
    if matches!(first.kind, TokenKind::Identifier | TokenKind::Keyword(_)) {
        Ok(start + 1)
    } else {
        Err(MacroExpansionError::new(
            first.span,
            "expected NWScript type",
        ))
    }
}

fn cursor_statement_end(trees: &[NwTokenTree], start: usize) -> Result<usize, MacroExpansionError> {
    if matches!(trees.get(start), Some(NwTokenTree::Group(group)) if group.delimiter == NwDelimiter::Brace)
    {
        return Ok(start + 1);
    }
    let first_text = trees.get(start).map(macro_tree_text).unwrap_or_default();
    if first_text == "if" {
        let then_start = cursor_control_body_start(trees, start)?;
        let then_end = cursor_statement_end(trees, then_start)?;
        if trees
            .get(then_end)
            .is_some_and(|tree| macro_tree_text(tree) == "else")
        {
            return cursor_statement_end(trees, then_end.saturating_add(1));
        }
        return Ok(then_end);
    }
    if matches!(first_text.as_str(), "while" | "for" | "switch") {
        let body_start = cursor_control_body_start(trees, start)?;
        return cursor_statement_end(trees, body_start);
    }
    if first_text == "do" {
        let body_end = cursor_statement_end(trees, start.saturating_add(1))?;
        let valid_tail = trees
            .get(body_end)
            .is_some_and(|tree| macro_tree_text(tree) == "while")
            && matches!(trees.get(body_end + 1), Some(NwTokenTree::Group(group)) if group.delimiter == NwDelimiter::Parenthesis)
            && matches!(trees.get(body_end + 2), Some(NwTokenTree::Token(token)) if token.kind == TokenKind::Semicolon);
        if valid_tail {
            return Ok(body_end + 3);
        }
        return Err(expected_cursor_fragment(
            trees.get(start).ok_or_else(|| {
                MacroExpansionError::without_span("expected NWScript do/while statement")
            })?,
            CursorFragmentKind::Statement,
        ));
    }
    for (index, tree) in trees.iter().enumerate().skip(start) {
        match tree {
            NwTokenTree::Token(token) if token.kind == TokenKind::Semicolon => {
                return Ok(index + 1);
            }
            _ => {}
        }
    }
    Err(expected_cursor_fragment(
        trees
            .get(start)
            .ok_or_else(|| MacroExpansionError::without_span("expected NWScript statement"))?,
        CursorFragmentKind::Statement,
    ))
}

fn cursor_control_body_start(
    trees: &[NwTokenTree],
    start: usize,
) -> Result<usize, MacroExpansionError> {
    if matches!(trees.get(start + 1), Some(NwTokenTree::Group(group)) if group.delimiter == NwDelimiter::Parenthesis)
    {
        Ok(start + 2)
    } else {
        Err(expected_cursor_fragment(
            trees.get(start).ok_or_else(|| {
                MacroExpansionError::without_span("expected NWScript control statement")
            })?,
            CursorFragmentKind::Statement,
        ))
    }
}

fn cursor_body_item_end(
    trees: &[NwTokenTree],
    start: usize,
    require_struct: bool,
) -> Result<usize, MacroExpansionError> {
    let first = trees
        .get(start)
        .ok_or_else(|| MacroExpansionError::without_span("expected body item"))?;
    if require_struct && macro_tree_text(first) != "struct" {
        return Err(expected_cursor_fragment(first, CursorFragmentKind::Struct));
    }
    let body_end = trees
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, tree)| {
            matches!(tree, NwTokenTree::Group(group) if group.delimiter == NwDelimiter::Brace)
                .then_some(index + 1)
        })
        .ok_or_else(|| {
            expected_cursor_fragment(
                first,
                if require_struct {
                    CursorFragmentKind::Struct
                } else {
                    CursorFragmentKind::Function
                },
            )
        })?;
    if !require_struct {
        return Ok(body_end);
    }
    if matches!(trees.get(body_end), Some(NwTokenTree::Token(token)) if token.kind == TokenKind::Semicolon)
    {
        Ok(body_end + 1)
    } else {
        Err(MacroExpansionError::new(
            first.span(),
            "NWScript struct declaration requires a trailing `;`",
        ))
    }
}

fn validate_cursor_fragment(
    stream: &NwTokenStream,
    kind: CursorFragmentKind,
    source_id: SourceId,
) -> Result<(), MacroExpansionError> {
    let source = crate::render_nwscript_tokens_bytes(stream);
    let wrapped = match kind {
        CursorFragmentKind::Expression => {
            let mut wrapped = b"void main() { ".to_vec();
            wrapped.extend_from_slice(&source);
            wrapped.extend_from_slice(b"; }");
            wrapped
        }
        CursorFragmentKind::Statement => {
            let mut wrapped = b"void main() { ".to_vec();
            wrapped.extend_from_slice(&source);
            wrapped.extend_from_slice(b" }");
            wrapped
        }
        CursorFragmentKind::Function | CursorFragmentKind::Struct => source,
        CursorFragmentKind::Identifier
        | CursorFragmentKind::Literal
        | CursorFragmentKind::Tree
        | CursorFragmentKind::Path
        | CursorFragmentKind::Type => return Ok(()),
    };
    let tokens = crate::lex_bytes(source_id, &wrapped).map_err(|error| {
        MacroExpansionError::without_span(format!("failed to lex {}: {error}", kind.name()))
    })?;
    crate::parse_tokens(tokens, None).map_err(|error| {
        MacroExpansionError::new(
            stream
                .trees()
                .first()
                .map_or(Span::new(source_id, 0, 0), NwTokenTree::span),
            format!("invalid {}: {error}", kind.name()),
        )
    })?;
    Ok(())
}

pub(super) fn macro_index(index: i32, script: &crate::VmScript) -> Result<usize, crate::VmError> {
    usize::try_from(index).map_err(|error| crate::VmError::Setup {
        message: format!(
            "negative tokenstream index {index} at instruction {}: {error}",
            script.ip()
        ),
    })
}

pub(super) fn pop_single_macro_tree(
    script: &mut crate::VmScript,
    arena: &RefCell<TokenStreamArena>,
) -> Result<NwTokenTree, crate::VmError> {
    let handle = pop_macro_tokenstream_handle(script)?;
    let arena = arena.borrow();
    let stream = arena.balanced(handle).map_err(macro_vm_error)?;
    let [tree] = stream.trees() else {
        return Err(crate::VmError::Setup {
            message: format!(
                "token inspection requires exactly one token tree, received {}",
                stream.len()
            ),
        });
    };
    Ok(tree.clone())
}

pub(super) fn macro_tree_kind(tree: &NwTokenTree) -> &'static str {
    match tree {
        NwTokenTree::Group(_) => "group",
        NwTokenTree::Token(token) => match token.kind {
            TokenKind::Eof => "eof",
            TokenKind::Identifier => "identifier",
            TokenKind::Integer => "integer",
            TokenKind::HexInteger => "hex_integer",
            TokenKind::BinaryInteger => "binary_integer",
            TokenKind::OctalInteger => "octal_integer",
            TokenKind::Float => "float",
            TokenKind::String => "string",
            TokenKind::HashedString => "hashed_string",
            TokenKind::Keyword(_) => "keyword",
            _ => "punctuation",
        },
    }
}
