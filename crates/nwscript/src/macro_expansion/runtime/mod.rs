use super::*;

mod cursor;
mod ordering_graph;

use cursor::*;
use ordering_graph::{EdgeStatus, OrderingGraph};

#[derive(Debug, Clone)]
enum TokenStreamListValue {
    Stream(u32),
    List(u32),
}

#[derive(Debug, Clone)]
struct TokenCursorState {
    stream:   NwTokenStream,
    position: usize,
}

#[derive(Debug)]
pub(super) struct TokenStreamArena {
    streams:         Vec<Vec<Token>>,
    lists:           Vec<Vec<TokenStreamListValue>>,
    quote_bindings:  Vec<QuoteBindings>,
    cursors:         Vec<TokenCursorState>,
    ordering_graphs: Vec<OrderingGraph>,
    limits:          MacroExpansionOptions,
    token_count:     usize,
    payload_bytes:   usize,
    list_items:      usize,
    graph_items:     usize,
}

impl TokenStreamArena {
    pub(super) fn with_input(
        input: NwTokenStream,
        limits: MacroExpansionOptions,
    ) -> Result<Self, crate::VmError> {
        let tokens = input.clone().into_tokens();
        let mut arena = Self {
            streams: Vec::new(),
            lists: vec![Vec::new()],
            quote_bindings: vec![QuoteBindings::new()],
            cursors: Vec::new(),
            ordering_graphs: Vec::new(),
            limits,
            token_count: 0,
            payload_bytes: 0,
            list_items: 0,
            graph_items: 0,
        };
        arena.insert_tokens(tokens)?;
        arena.insert_cursor(input)?;
        Ok(arena)
    }

    fn insert(&mut self, stream: NwTokenStream) -> Result<u32, crate::VmError> {
        self.insert_tokens(stream.into_tokens())
    }

    fn insert_tokens(&mut self, tokens: Vec<Token>) -> Result<u32, crate::VmError> {
        self.ensure_handle_capacity(1)?;
        self.ensure_token_capacity(tokens.len(), token_slice_storage_bytes(&tokens))?;
        let handle = u32::try_from(self.streams.len()).map_err(|error| crate::VmError::Setup {
            message: format!("compiler tokenstream arena exhausted: {error}"),
        })?;
        self.token_count += tokens.len();
        self.payload_bytes += token_slice_storage_bytes(&tokens);
        self.streams.push(tokens);
        Ok(handle)
    }

    fn get(&self, handle: u32) -> Option<&[Token]> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.streams.get(index))
            .map(Vec::as_slice)
    }

    pub(super) fn balanced(&self, handle: u32) -> Result<NwTokenStream, MacroExpansionError> {
        let tokens = self.get(handle).ok_or_else(|| {
            MacroExpansionError::without_span(format!(
                "unknown compiler tokenstream handle {handle}"
            ))
        })?;
        NwTokenStream::from_tokens(tokens)
    }

    fn insert_list(&mut self, values: Vec<TokenStreamListValue>) -> Result<u32, crate::VmError> {
        self.ensure_handle_capacity(1)?;
        self.ensure_list_capacity(values.len())?;
        let handle = u32::try_from(self.lists.len()).map_err(|error| crate::VmError::Setup {
            message: format!("compiler tokenstream-list arena exhausted: {error}"),
        })?;
        self.list_items += values.len();
        self.lists.push(values);
        Ok(handle)
    }

    fn list(&self, handle: u32) -> Option<&[TokenStreamListValue]> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.lists.get(index))
            .map(Vec::as_slice)
    }

    fn insert_quote_bindings(&mut self, bindings: QuoteBindings) -> Result<u32, crate::VmError> {
        self.ensure_handle_capacity(1)?;
        let (tokens, bytes) = quote_bindings_storage(&bindings);
        self.ensure_token_capacity(tokens, bytes)?;
        let handle =
            u32::try_from(self.quote_bindings.len()).map_err(|error| crate::VmError::Setup {
                message: format!("compiler quote-binding arena exhausted: {error}"),
            })?;
        self.token_count += tokens;
        self.payload_bytes += bytes;
        self.quote_bindings.push(bindings);
        Ok(handle)
    }

    fn bindings(&self, handle: u32) -> Option<&QuoteBindings> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.quote_bindings.get(index))
    }

    fn binding_from_list(&self, handle: u32) -> Result<QuoteBinding, MacroExpansionError> {
        let values = self.list(handle).ok_or_else(|| {
            MacroExpansionError::without_span(format!(
                "unknown compiler tokenstream-list handle {handle}"
            ))
        })?;
        let mut bindings = Vec::with_capacity(values.len());
        for value in values {
            bindings.push(match value {
                TokenStreamListValue::Stream(handle) => {
                    QuoteBinding::Single(self.balanced(*handle)?)
                }
                TokenStreamListValue::List(handle) => self.binding_from_list(*handle)?,
            });
        }
        if bindings
            .iter()
            .all(|binding| matches!(binding, QuoteBinding::Single(_)))
        {
            Ok(QuoteBinding::Repeated(
                bindings
                    .into_iter()
                    .filter_map(|binding| match binding {
                        QuoteBinding::Single(stream) => Some(stream),
                        QuoteBinding::Repeated(_) | QuoteBinding::Nested(_) => None,
                    })
                    .collect(),
            ))
        } else {
            Ok(QuoteBinding::Nested(bindings))
        }
    }

    fn insert_cursor(&mut self, stream: NwTokenStream) -> Result<u32, crate::VmError> {
        self.ensure_handle_capacity(1)?;
        let (tokens, bytes) = token_stream_storage(&stream);
        self.ensure_token_capacity(tokens, bytes)?;
        let handle = u32::try_from(self.cursors.len()).map_err(|error| crate::VmError::Setup {
            message: format!("compiler token-cursor arena exhausted: {error}"),
        })?;
        self.token_count += tokens;
        self.payload_bytes += bytes;
        self.cursors.push(TokenCursorState {
            stream,
            position: 0,
        });
        Ok(handle)
    }

    fn cursor(&self, handle: u32) -> Option<&TokenCursorState> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.cursors.get(index))
    }

    fn cursor_mut(&mut self, handle: u32) -> Option<&mut TokenCursorState> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.cursors.get_mut(index))
    }

    fn insert_ordering_graph(&mut self, graph: OrderingGraph) -> Result<u32, crate::VmError> {
        self.ensure_handle_capacity(1)?;
        let handle =
            u32::try_from(self.ordering_graphs.len()).map_err(|error| crate::VmError::Setup {
                message: format!("compiler ordering-graph arena exhausted: {error}"),
            })?;
        self.ordering_graphs.push(graph);
        Ok(handle)
    }

    fn ordering_graph(&self, handle: u32) -> Option<&OrderingGraph> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.ordering_graphs.get(index))
    }

    fn ordering_graph_mut(&mut self, handle: u32) -> Option<&mut OrderingGraph> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.ordering_graphs.get_mut(index))
    }

    fn handle_count(&self) -> usize {
        self.streams.len()
            + self.lists.len()
            + self.quote_bindings.len()
            + self.cursors.len()
            + self.ordering_graphs.len()
    }

    fn ensure_handle_capacity(&self, additional: usize) -> Result<(), crate::VmError> {
        let requested = self.handle_count().checked_add(additional).ok_or_else(|| {
            macro_arena_limit_error("handle", usize::MAX, self.limits.arena_handle_limit)
        })?;
        if requested > self.limits.arena_handle_limit {
            return Err(macro_arena_limit_error(
                "handle",
                requested,
                self.limits.arena_handle_limit,
            ));
        }
        Ok(())
    }

    fn ensure_token_capacity(
        &self,
        additional_tokens: usize,
        additional_bytes: usize,
    ) -> Result<(), crate::VmError> {
        let tokens = self
            .token_count
            .checked_add(additional_tokens)
            .ok_or_else(|| {
                macro_arena_limit_error("token", usize::MAX, self.limits.arena_token_limit)
            })?;
        if tokens > self.limits.arena_token_limit {
            return Err(macro_arena_limit_error(
                "token",
                tokens,
                self.limits.arena_token_limit,
            ));
        }
        let bytes = self
            .payload_bytes
            .checked_add(additional_bytes)
            .ok_or_else(|| {
                macro_arena_limit_error("byte", usize::MAX, self.limits.arena_byte_limit)
            })?;
        if bytes > self.limits.arena_byte_limit {
            return Err(macro_arena_limit_error(
                "byte",
                bytes,
                self.limits.arena_byte_limit,
            ));
        }
        Ok(())
    }

    fn ensure_list_capacity(&self, additional: usize) -> Result<(), crate::VmError> {
        let requested = self
            .list_items
            .checked_add(self.graph_items)
            .and_then(|current| current.checked_add(additional))
            .ok_or_else(|| {
                macro_arena_limit_error("list item", usize::MAX, self.limits.arena_list_item_limit)
            })?;
        if requested > self.limits.arena_list_item_limit {
            return Err(macro_arena_limit_error(
                "list item",
                requested,
                self.limits.arena_list_item_limit,
            ));
        }
        Ok(())
    }

    fn ensure_graph_capacity(&self, additional: usize) -> Result<(), crate::VmError> {
        self.ensure_list_capacity(additional)
    }
}

fn macro_arena_limit_error(kind: &str, requested: usize, limit: usize) -> crate::VmError {
    crate::VmError::Setup {
        message: format!(
            "procedural macro arena {kind} budget exceeded: requested {requested}, limit {limit}"
        ),
    }
}

fn token_slice_storage_bytes(tokens: &[Token]) -> usize {
    tokens.iter().fold(0usize, |total, token| {
        total.saturating_add(std::mem::size_of::<Token>() + token.text.len())
    })
}

fn token_stream_storage(stream: &NwTokenStream) -> (usize, usize) {
    fn tree_storage(tree: &NwTokenTree) -> (usize, usize) {
        match tree {
            NwTokenTree::Token(token) => (
                1,
                std::mem::size_of::<Token>().saturating_add(token.text.len()),
            ),
            NwTokenTree::Group(group) => group.stream.trees.iter().fold(
                (0usize, 0usize),
                |(token_total, byte_total), tree| {
                    let (tokens, bytes) = tree_storage(tree);
                    (
                        token_total.saturating_add(tokens),
                        byte_total.saturating_add(bytes),
                    )
                },
            ),
        }
    }

    stream
        .trees
        .iter()
        .fold((0usize, 0usize), |(token_total, byte_total), tree| {
            let (tokens, bytes) = tree_storage(tree);
            (
                token_total.saturating_add(tokens),
                byte_total.saturating_add(bytes),
            )
        })
}

fn quote_bindings_storage(bindings: &QuoteBindings) -> (usize, usize) {
    fn binding_storage(binding: &QuoteBinding) -> (usize, usize) {
        match binding {
            QuoteBinding::Single(stream) => token_stream_storage(stream),
            QuoteBinding::Repeated(streams) => {
                streams
                    .iter()
                    .fold((0usize, 0usize), |(token_total, byte_total), stream| {
                        let (tokens, bytes) = token_stream_storage(stream);
                        (
                            token_total.saturating_add(tokens),
                            byte_total.saturating_add(bytes),
                        )
                    })
            }
            QuoteBinding::Nested(bindings) => {
                bindings
                    .iter()
                    .fold((0usize, 0usize), |(token_total, byte_total), binding| {
                        let (tokens, bytes) = binding_storage(binding);
                        (
                            token_total.saturating_add(tokens),
                            byte_total.saturating_add(bytes),
                        )
                    })
            }
        }
    }

    bindings
        .values
        .values()
        .fold((0usize, 0usize), |(token_total, byte_total), binding| {
            let (tokens, bytes) = binding_storage(binding);
            (
                token_total.saturating_add(tokens),
                byte_total.saturating_add(bytes),
            )
        })
}

pub(super) fn macro_vm(arena: Rc<RefCell<TokenStreamArena>>, source_id: SourceId) -> crate::Vm {
    let mut vm = crate::Vm::new();
    vm.define_engine_structure_default(
        MACRO_TOKENSTREAM_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    vm.define_engine_structure_default(
        MACRO_TOKENSTREAM_LIST_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    vm.define_engine_structure_default(
        MACRO_QUOTE_BINDINGS_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    vm.define_engine_structure_default(
        MACRO_TOKEN_CURSOR_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    vm.define_engine_structure_default(
        MACRO_ORDERING_GRAPH_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(0, move |script| {
            let source = script.pop_string()?;
            let tokens = crate::lex_bytes(source_id, source.as_bytes()).map_err(|error| {
                crate::VmError::Setup {
                    message: format!("could not lex quoted NWScript: {error}"),
                }
            })?;
            let tokens = tokens
                .into_iter()
                .filter(|token| token.kind != TokenKind::Eof)
                .collect();
            push_macro_tokenstream(script, arena.borrow_mut().insert_tokens(tokens)?);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(1, move |script| {
            let left = pop_macro_tokenstream_handle(script)?;
            let right = pop_macro_tokenstream_handle(script)?;
            let mut arena = arena.borrow_mut();
            let left_tokens = arena
                .get(left)
                .ok_or_else(|| unknown_tokenstream_handle(script, left))?;
            let right_tokens = arena
                .get(right)
                .ok_or_else(|| unknown_tokenstream_handle(script, right))?;
            let combined_len = left_tokens
                .len()
                .checked_add(right_tokens.len())
                .ok_or_else(|| {
                    macro_arena_limit_error("token", usize::MAX, arena.limits.arena_token_limit)
                })?;
            let combined_bytes = token_slice_storage_bytes(left_tokens)
                .checked_add(token_slice_storage_bytes(right_tokens))
                .ok_or_else(|| {
                    macro_arena_limit_error("byte", usize::MAX, arena.limits.arena_byte_limit)
                })?;
            arena.ensure_handle_capacity(1)?;
            arena.ensure_token_capacity(combined_len, combined_bytes)?;
            let mut combined = Vec::with_capacity(combined_len);
            combined.extend_from_slice(left_tokens);
            combined.extend_from_slice(right_tokens);
            let handle = arena.insert_tokens(combined)?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(2, move |script| {
            let handle = arena.borrow_mut().insert(NwTokenStream::new())?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    define_tokenstream_inspection_commands(&mut vm, Rc::clone(&arena), source_id);
    define_tokenstream_collection_commands(&mut vm, Rc::clone(&arena), source_id);
    define_token_cursor_commands(&mut vm, Rc::clone(&arena), source_id);
    define_ordering_graph_commands(&mut vm, Rc::clone(&arena));
    define_token_project_commands(&mut vm, arena);
    vm
}

fn pop_macro_tokenstream_handle(script: &mut crate::VmScript) -> Result<u32, crate::VmError> {
    let value = script.pop_engine_structure_index(MACRO_TOKENSTREAM_INDEX)?;
    match value {
        crate::VmEngineStructureValue::Word(handle) => Ok(handle),
        crate::VmEngineStructureValue::Text(_) => Err(crate::VmError::TypeMismatch {
            offset:   script.ip(),
            message:  "expected handle-backed compiler tokenstream".to_string(),
            expected: Some("engine structure"),
            actual:   "engine structure",
        }),
    }
}

fn push_macro_tokenstream(script: &mut crate::VmScript, handle: u32) {
    script.push_engine_structure(
        MACRO_TOKENSTREAM_INDEX,
        crate::VmEngineStructureValue::Word(handle),
    );
}

fn pop_macro_handle(
    script: &mut crate::VmScript,
    index: u8,
    name: &'static str,
) -> Result<u32, crate::VmError> {
    match script.pop_engine_structure_index(index)? {
        crate::VmEngineStructureValue::Word(handle) => Ok(handle),
        crate::VmEngineStructureValue::Text(_) => Err(crate::VmError::TypeMismatch {
            offset:   script.ip(),
            message:  format!("expected handle-backed compiler {name}"),
            expected: Some("engine structure"),
            actual:   "engine structure",
        }),
    }
}

fn push_macro_handle(script: &mut crate::VmScript, index: u8, handle: u32) {
    script.push_engine_structure(index, crate::VmEngineStructureValue::Word(handle));
}

fn unknown_tokenstream_handle(script: &crate::VmScript, handle: u32) -> crate::VmError {
    crate::VmError::Setup {
        message: format!(
            "unknown compiler tokenstream handle {handle} at instruction {}",
            script.ip()
        ),
    }
}

fn macro_vm_error(error: MacroExpansionError) -> crate::VmError {
    crate::VmError::Setup {
        message: error.to_string(),
    }
}

fn pop_macro_ascii(
    script: &mut crate::VmScript,
    label: &'static str,
) -> Result<String, crate::VmError> {
    let value = script.pop_string()?.into_bytes();
    if !value.is_ascii() {
        return Err(crate::VmError::Setup {
            message: format!("{label} must contain only ASCII bytes"),
        });
    }
    String::from_utf8(value).map_err(|error| crate::VmError::Setup {
        message: format!("invalid {label}: {error}"),
    })
}

fn pop_macro_token_text(script: &mut crate::VmScript) -> Result<String, crate::VmError> {
    Ok(script
        .pop_string()?
        .into_bytes()
        .into_iter()
        .map(char::from)
        .collect())
}

fn define_tokenstream_inspection_commands(
    vm: &mut crate::Vm,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(3, move |script| {
            let handle = pop_macro_tokenstream_handle(script)?;
            let length = arena
                .borrow()
                .balanced(handle)
                .map_err(macro_vm_error)?
                .len();
            let length = i32::try_from(length).map_err(|error| crate::VmError::Setup {
                message: format!("tokenstream length exceeds NWScript integer range: {error}"),
            })?;
            script.push_int(length);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(4, move |script| {
            let stream_handle = pop_macro_tokenstream_handle(script)?;
            let index = macro_index(script.pop_int()?, script)?;
            let tree = arena
                .borrow()
                .balanced(stream_handle)
                .map_err(macro_vm_error)?
                .trees()
                .get(index)
                .cloned()
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("tokenstream index {index} is out of bounds"),
                })?;
            let handle = arena
                .borrow_mut()
                .insert(NwTokenStream::from_trees(vec![tree]))?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(5, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            script.push_int(i32::from(matches!(tree, NwTokenTree::Group(_))));
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(6, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            script.push_string(crate::ScriptString::new(
                macro_tree_kind(&tree).as_bytes().to_vec(),
            ));
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(7, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            let text = match tree {
                NwTokenTree::Token(token) => crate::ScriptString::from_lexed_text(&token.text),
                tree @ NwTokenTree::Group(_) => crate::ScriptString::new(
                    crate::render_nwscript_tokens_bytes(&NwTokenStream::from_trees(vec![tree])),
                ),
            };
            script.push_string(text);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(8, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            let delimiter = match tree {
                NwTokenTree::Token(_) => 0,
                NwTokenTree::Group(group) => match group.delimiter {
                    NwDelimiter::Parenthesis => 1,
                    NwDelimiter::Bracket => 2,
                    NwDelimiter::Brace => 3,
                },
            };
            script.push_int(delimiter);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(9, move |script| {
            let source = script.pop_string()?;
            let tokens = crate::lex_bytes(source_id, source.as_bytes()).map_err(|error| {
                crate::VmError::Setup {
                    message: format!("could not lex parsed tokenstream source: {error}"),
                }
            })?;
            let stream = NwTokenStream::from_tokens(&tokens).map_err(macro_vm_error)?;
            let handle = arena.borrow_mut().insert(stream)?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    vm.define_simple_command(10, |script| {
        let message = script.pop_string()?;
        Err(crate::VmError::Setup {
            message: format!("procedural macro reported: {}", message.display_text()),
        })
    });
}

fn define_tokenstream_collection_commands(
    vm: &mut crate::Vm,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(11, move |script| {
            let handle = arena.borrow_mut().insert_list(Vec::new())?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(12, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let stream = pop_macro_tokenstream_handle(script)?;
            let mut arena = arena.borrow_mut();
            let existing = arena.list(list).ok_or_else(|| crate::VmError::Setup {
                message: format!("unknown compiler tokenstream-list handle {list}"),
            })?;
            let requested = existing.len().checked_add(1).ok_or_else(|| {
                macro_arena_limit_error("list item", usize::MAX, arena.limits.arena_list_item_limit)
            })?;
            arena.ensure_handle_capacity(1)?;
            arena.ensure_list_capacity(requested)?;
            let mut values = existing.to_vec();
            values.push(TokenStreamListValue::Stream(stream));
            let handle = arena.insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(13, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let nested =
                pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let mut arena = arena.borrow_mut();
            if arena.list(nested).is_none() {
                return Err(crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {nested}"),
                });
            }
            let existing = arena.list(list).ok_or_else(|| crate::VmError::Setup {
                message: format!("unknown compiler tokenstream-list handle {list}"),
            })?;
            let requested = existing.len().checked_add(1).ok_or_else(|| {
                macro_arena_limit_error("list item", usize::MAX, arena.limits.arena_list_item_limit)
            })?;
            arena.ensure_handle_capacity(1)?;
            arena.ensure_list_capacity(requested)?;
            let mut values = existing.to_vec();
            values.push(TokenStreamListValue::List(nested));
            let handle = arena.insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(14, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let length = arena
                .borrow()
                .list(list)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {list}"),
                })?
                .len();
            script.push_int(
                i32::try_from(length).map_err(|error| crate::VmError::Setup {
                    message: format!(
                        "tokenstream-list length exceeds NWScript integer range: {error}"
                    ),
                })?,
            );
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(15, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let index = macro_index(script.pop_int()?, script)?;
            let arena = arena.borrow();
            let value = arena
                .list(list)
                .and_then(|values| values.get(index))
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("tokenstream-list index {index} is out of bounds"),
                })?;
            let TokenStreamListValue::Stream(handle) = value else {
                return Err(crate::VmError::Setup {
                    message: format!("tokenstream-list entry {index} is a nested list"),
                });
            };
            push_macro_tokenstream(script, *handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(16, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let index = macro_index(script.pop_int()?, script)?;
            let arena = arena.borrow();
            let value = arena
                .list(list)
                .and_then(|values| values.get(index))
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("tokenstream-list index {index} is out of bounds"),
                })?;
            let TokenStreamListValue::List(handle) = value else {
                return Err(crate::VmError::Setup {
                    message: format!("tokenstream-list entry {index} is not a nested list"),
                });
            };
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, *handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(17, move |script| {
            let handle = arena
                .borrow_mut()
                .insert_quote_bindings(QuoteBindings::new())?;
            push_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(18, move |script| {
            let bindings = pop_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, "quote bindings")?;
            let name = pop_macro_ascii(script, "quote binding name")?;
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let mut arena = arena.borrow_mut();
            let mut bindings =
                arena
                    .bindings(bindings)
                    .cloned()
                    .ok_or_else(|| crate::VmError::Setup {
                        message: format!("unknown compiler quote-binding handle {bindings}"),
                    })?;
            let binding = arena.binding_from_list(list).map_err(macro_vm_error)?;
            bindings.values.insert(name, binding);
            let handle = arena.insert_quote_bindings(bindings)?;
            push_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, handle);
            Ok(())
        });
    }
    vm.define_simple_command(19, move |script| {
        let template = script.pop_string()?.into_bytes();
        let bindings = pop_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, "quote bindings")?;
        let separator = script.pop_string()?.into_bytes();
        let quantifier = match script.pop_int()? {
            0 => b'*',
            1 => b'+',
            2 => b'?',
            value => {
                return Err(crate::VmError::Setup {
                    message: format!("unknown quote repetition quantifier {value}"),
                });
            }
        };
        let mut source = Vec::with_capacity(
            template
                .len()
                .saturating_add(separator.len())
                .saturating_add(4),
        );
        source.extend_from_slice(b"$(");
        source.extend_from_slice(&template);
        source.push(b')');
        source.extend_from_slice(&separator);
        source.push(quantifier);
        let tokens =
            crate::lex_bytes(source_id, &source).map_err(|error| crate::VmError::Setup {
                message: format!("could not lex quote repetition: {error}"),
            })?;
        let quoted = {
            let arena = arena.borrow();
            let bindings = arena
                .bindings(bindings)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler quote-binding handle {bindings}"),
                })?;
            let template = NwTokenStream::from_tokens(&tokens).map_err(macro_vm_error)?;
            quote_nwscript(&template, bindings).map_err(macro_vm_error)?
        };
        let handle = arena.borrow_mut().insert(quoted)?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

fn define_ordering_graph_commands(vm: &mut crate::Vm, arena: Rc<RefCell<TokenStreamArena>>) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(41, move |script| {
            let handle = arena
                .borrow_mut()
                .insert_ordering_graph(OrderingGraph::default())?;
            push_macro_handle(script, MACRO_ORDERING_GRAPH_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(42, move |script| {
            let graph = pop_macro_handle(script, MACRO_ORDERING_GRAPH_INDEX, "ordering graph")?;
            let key = pop_macro_ascii(script, "ordering-graph node key")?;
            let payload = pop_macro_tokenstream_handle(script)?;
            let diagnostic = pop_macro_tokenstream_handle(script)?;
            let order =
                usize::try_from(script.pop_int()?).map_err(|_error| crate::VmError::Setup {
                    message: "ordering-graph source order must be non-negative".to_string(),
                })?;
            let mut arena = arena.borrow_mut();
            if arena
                .ordering_graph(graph)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler ordering-graph handle {graph}"),
                })?
                .contains(&key)
            {
                return Err(crate::VmError::Setup {
                    message: format!("duplicate compiler ordering-graph node `{key}`"),
                });
            }
            arena.ensure_graph_capacity(1)?;
            let inserted = arena
                .ordering_graph_mut(graph)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler ordering-graph handle {graph}"),
                })?
                .add_node(key, payload, diagnostic, order);
            debug_assert!(inserted);
            arena.graph_items += 1;
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(43, move |script| {
            let graph = pop_macro_handle(script, MACRO_ORDERING_GRAPH_INDEX, "ordering graph")?;
            let key = pop_macro_ascii(script, "ordering-graph node key")?;
            let contains = arena
                .borrow()
                .ordering_graph(graph)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler ordering-graph handle {graph}"),
                })?
                .contains(&key);
            script.push_int(i32::from(contains));
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(44, move |script| {
            let graph = pop_macro_handle(script, MACRO_ORDERING_GRAPH_INDEX, "ordering graph")?;
            let before = pop_macro_ascii(script, "ordering-graph edge source")?;
            let after = pop_macro_ascii(script, "ordering-graph edge target")?;
            let mut arena = arena.borrow_mut();
            let status = arena
                .ordering_graph(graph)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler ordering-graph handle {graph}"),
                })?
                .edge_status(&before, &after)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!(
                        "ordering-graph edge references an unknown node: `{before}` -> `{after}`"
                    ),
                })?;
            if status == EdgeStatus::Added {
                arena.ensure_graph_capacity(1)?;
                let added = arena
                    .ordering_graph_mut(graph)
                    .ok_or_else(|| crate::VmError::Setup {
                        message: format!("unknown compiler ordering-graph handle {graph}"),
                    })?
                    .add_edge(before, after);
                debug_assert_eq!(added, Some(EdgeStatus::Added));
                arena.graph_items += 1;
            }
            script.push_int(match status {
                EdgeStatus::Added => 1,
                EdgeStatus::Duplicate => 0,
                EdgeStatus::Contradiction => -1,
            });
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(45, move |script| {
            let graph = pop_macro_handle(script, MACRO_ORDERING_GRAPH_INDEX, "ordering graph")?;
            let values = arena
                .borrow()
                .ordering_graph(graph)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler ordering-graph handle {graph}"),
                })?
                .stable_sort()
                .unwrap_or_default()
                .into_iter()
                .map(TokenStreamListValue::Stream)
                .collect::<Vec<_>>();
            let handle = arena.borrow_mut().insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(46, move |script| {
            let graph = pop_macro_handle(script, MACRO_ORDERING_GRAPH_INDEX, "ordering graph")?;
            let values = arena
                .borrow()
                .ordering_graph(graph)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler ordering-graph handle {graph}"),
                })?
                .cycle()
                .into_iter()
                .map(TokenStreamListValue::Stream)
                .collect::<Vec<_>>();
            let handle = arena.borrow_mut().insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(47, move |script| {
            let left = pop_macro_tokenstream_handle(script)?;
            let right = pop_macro_tokenstream_handle(script)?;
            let arena = arena.borrow();
            let left = arena.balanced(left).map_err(macro_vm_error)?;
            let right = arena.balanced(right).map_err(macro_vm_error)?;
            let equal = match (left.trees(), right.trees()) {
                ([left], [right]) => left.span() == right.span(),
                _ => false,
            };
            script.push_int(i32::from(equal));
            Ok(())
        });
    }
}
