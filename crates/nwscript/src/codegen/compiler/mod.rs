use super::*;

mod expressions;
mod globals;
mod statements;

use globals::GlobalEmitter;

pub(super) struct O0Compiler<'a> {
    pub(super) hir:                   &'a HirModule,
    pub(super) langspec:              Option<&'a LangSpec>,
    pub(super) builtin_functions:     BTreeMap<String, (u16, &'a BuiltinFunction)>,
    pub(super) builtin_constants:     BTreeMap<String, BuiltinValue>,
    pub(super) constant_env:          BTreeMap<String, ConstValue>,
    pub(super) structs:               BTreeMap<String, &'a crate::HirStruct>,
    pub(super) functions:             BTreeMap<String, &'a HirFunction>,
    pub(super) entry_function:        Option<&'a HirFunction>,
    pub(super) global_layout:         BTreeMap<String, ValueLayout>,
    pub(super) global_size:           usize,
    pub(super) global_init_bytes:     Option<usize>,
    pub(super) function_labels:       BTreeMap<String, LabelId>,
    pub(super) function_exit_labels:  BTreeMap<String, LabelId>,
    pub(super) function_end_labels:   BTreeMap<String, LabelId>,
    pub(super) globals_label:         Option<LabelId>,
    pub(super) globals_end_label:     Option<LabelId>,
    pub(super) variable_debug:        Vec<VariableDebugInfo>,
    pub(super) line_debug:            LineDebugTracker,
    pub(super) source_map:            Option<&'a SourceMap>,
    pub(super) current_function_name: Option<&'a str>,
    pub(super) cancellation:          Option<&'a CancellationToken>,
    pub(super) compile_time:          SystemTime,
    pub(super) assembler:             Assembler,
}

#[derive(Clone)]
pub(super) struct ValueLayout {
    pub(super) offset: usize,
    pub(super) size:   usize,
}

#[derive(Clone)]
pub(super) struct FieldLayout {
    pub(super) ty:     SemanticType,
    pub(super) offset: usize,
    pub(super) size:   usize,
}

#[derive(Clone)]
pub(super) struct FunctionLayout {
    pub(super) return_layout:      Option<ValueLayout>,
    pub(super) locals:             BTreeMap<HirLocalId, ValueLayout>,
    pub(super) parameter_size:     usize,
    pub(super) active_locals_size: usize,
}

pub(super) struct FunctionScope {
    pub(super) variable_debug: Vec<usize>,
    pub(super) locals:         Vec<HirLocalId>,
    pub(super) local_bytes:    usize,
    pub(super) temp_bytes:     usize,
}

#[derive(Clone, Copy)]
pub(super) struct ControlTarget {
    pub(super) label:       LabelId,
    pub(super) scope_depth: usize,
    pub(super) temp_bytes:  usize,
}

pub(super) struct FunctionEmitter<'a, 'b> {
    pub(super) compiler:         &'b mut O0Compiler<'a>,
    pub(super) function:         &'a HirFunction,
    pub(super) layout:           FunctionLayout,
    pub(super) temp_bytes:       usize,
    pub(super) break_targets:    Vec<ControlTarget>,
    pub(super) continue_targets: Vec<ControlTarget>,
    pub(super) scope_stack:      Vec<FunctionScope>,
}

impl<'a> O0Compiler<'a> {
    pub(super) fn new(
        hir: &'a HirModule,
        langspec: Option<&'a LangSpec>,
        source_map: Option<&'a SourceMap>,
    ) -> Result<Self, CodegenError> {
        Self::new_impl(hir, langspec, source_map, None)
    }

    pub(super) fn new_with_cancellation(
        hir: &'a HirModule,
        langspec: Option<&'a LangSpec>,
        source_map: Option<&'a SourceMap>,
        cancellation: &'a CancellationToken,
    ) -> Result<Self, CodegenError> {
        Self::new_impl(hir, langspec, source_map, Some(cancellation))
    }

    pub(super) fn new_impl(
        hir: &'a HirModule,
        langspec: Option<&'a LangSpec>,
        source_map: Option<&'a SourceMap>,
        cancellation: Option<&'a CancellationToken>,
    ) -> Result<Self, CodegenError> {
        let mut builtin_functions = BTreeMap::new();
        let mut builtin_constants = BTreeMap::new();
        if let Some(langspec) = langspec {
            for (index, function) in langspec.functions.iter().enumerate() {
                builtin_functions.insert(
                    function.name.clone(),
                    (usize_to_u16(index, "builtin function index")?, function),
                );
            }
            for constant in &langspec.constants {
                builtin_constants.insert(constant.name.clone(), constant.value.clone());
            }
        }
        let constant_env = build_constant_env(hir, langspec);

        let structs = hir
            .structs
            .iter()
            .map(|structure| (structure.name.clone(), structure))
            .collect::<BTreeMap<_, _>>();
        let functions = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), function))
            .collect::<BTreeMap<_, _>>();
        let entry_function = functions
            .get("main")
            .copied()
            .or_else(|| functions.get("StartingConditional").copied());

        let mut global_layout = BTreeMap::new();
        let mut global_size = 0usize;
        for global in hir.globals.iter().filter(|global| !global.is_const) {
            let size = size_of_type(&global.ty, &structs)?;
            global_layout.insert(
                global.name.clone(),
                ValueLayout {
                    offset: global_size,
                    size,
                },
            );
            global_size += size;
        }

        let mut assembler = Assembler::default();
        // Native parsing places top-level structure definitions in the global
        // parse tree. They therefore retain the saved-base-pointer wrapper
        // even when no mutable global storage is allocated. Const-only units
        // do not create that wrapper.
        let globals_label =
            (global_size > 0 || !hir.structs.is_empty()).then(|| assembler.new_label());
        let globals_end_label = globals_label.map(|_| assembler.new_label());
        let function_labels = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), assembler.new_label()))
            .collect::<BTreeMap<_, _>>();
        let function_end_labels = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), assembler.new_label()))
            .collect::<BTreeMap<_, _>>();
        let function_exit_labels = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), assembler.new_label()))
            .collect::<BTreeMap<_, _>>();

        Ok(Self {
            hir,
            langspec,
            builtin_functions,
            builtin_constants,
            constant_env,
            structs,
            functions,
            entry_function,
            global_layout,
            global_size,
            global_init_bytes: None,
            function_labels,
            function_exit_labels,
            function_end_labels,
            globals_label,
            globals_end_label,
            variable_debug: Vec::new(),
            line_debug: LineDebugTracker::default(),
            source_map,
            current_function_name: None,
            cancellation,
            compile_time: SystemTime::now(),
            assembler,
        })
    }

    pub(super) fn compile(mut self, meld: bool) -> Result<CodegenOutput, CodegenError> {
        self.check_cancelled(None)?;
        self.emit_loader()?;

        if let Some(globals_label) = self.globals_label {
            self.assembler.place_label(globals_label);
            self.emit_globals()?;
            if let Some(end) = self.globals_end_label {
                self.assembler.place_label(end);
            }
        }

        for function in &self.hir.functions {
            self.check_cancelled(Some(function.span))?;
            if function.is_builtin {
                continue;
            }
            let label = self
                .function_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function label for {:?}", function.name),
                    )
                })?;
            self.assembler.place_label(label);
            self.emit_function(function)?;
            let end_label = self
                .function_end_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function end label for {:?}", function.name),
                    )
                })?;
            self.assembler.place_label(end_label);
        }

        if meld {
            self.check_cancelled(None)?;
            self.assembler.meld_instructions();
        }
        let assembly = self.assembler.finalize()?;
        Ok(CodegenOutput {
            instructions:  assembly.instructions,
            label_offsets: assembly.offsets,
            functions:     self
                .function_labels
                .iter()
                .map(|(name, start)| {
                    let end = self.function_end_labels.get(name).copied().ok_or_else(|| {
                        CodegenError::new(None, format!("missing function end label for {name:?}"))
                    })?;
                    Ok::<_, CodegenError>((
                        name.clone(),
                        FunctionDebugInfo {
                            start: *start,
                            end,
                        },
                    ))
                })
                .collect::<Result<_, _>>()?,
            variables:     self.variable_debug,
            lines:         self.line_debug.entries,
        })
    }

    pub(super) fn check_cancelled(&self, span: Option<crate::Span>) -> Result<(), CodegenError> {
        if self
            .cancellation
            .is_some_and(CancellationToken::is_cancelled)
        {
            Err(CodegenError::new(span, "operation cancelled"))
        } else {
            Ok(())
        }
    }

    pub(super) fn emit_loader(&mut self) -> Result<(), CodegenError> {
        if let Some(entry) = self.entry_function
            && entry.return_type != SemanticType::Void
        {
            self.emit_stack_alloc(&entry.return_type)?;
            let start = self.assembler.new_label();
            self.assembler.place_label(start);
            self.variable_debug.push(VariableDebugInfo {
                name: "#retval".to_string(),
                ty: entry.return_type.clone(),
                start,
                end: None,
                stack_loc: 0,
            });
        }

        if let Some(globals_label) = self.globals_label {
            self.assembler.push_jump(NcsOpcode::Jsr, globals_label);
        } else if let Some(entry) = self.entry_function {
            let label = self
                .function_labels
                .get(&entry.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(entry.span),
                        format!("missing function label for {:?}", entry.name),
                    )
                })?;
            self.assembler.push_jump(NcsOpcode::Jsr, label);
        }

        self.assembler.push(simple_instruction(NcsOpcode::Ret));
        Ok(())
    }

    pub(super) fn emit_globals(&mut self) -> Result<(), CodegenError> {
        let globals = self
            .hir
            .globals
            .iter()
            .filter(|global| !global.is_const)
            .cloned()
            .collect::<Vec<_>>();
        let mut emitter = GlobalEmitter {
            compiler:        self,
            allocated_bytes: 0,
            temp_bytes:      0,
        };
        for global in &globals {
            emitter.compiler.emit_stack_alloc(&global.ty)?;
            let size = size_of_type(&global.ty, &emitter.compiler.structs)?;
            emitter.allocated_bytes += size;
            let start = emitter.compiler.assembler.new_label();
            emitter.compiler.assembler.place_label(start);
            let layout = emitter
                .compiler
                .global_layout
                .get(&global.name)
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(global.span),
                        format!("unknown global {:?}", global.name),
                    )
                })?;
            emitter.compiler.variable_debug.push(VariableDebugInfo {
                name: global.name.clone(),
                ty: global.ty.clone(),
                start,
                end: None,
                stack_loc: usize_to_u32(layout.offset, "global stack location")?,
            });
            if let Some(initializer) = &global.initializer {
                let start = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(start);
                emitter.compiler.start_line_at(global.span, start);
                emitter.emit_expr(initializer)?;
                emitter.emit_store_global(&global.name, initializer.span)?;
                emitter.emit_pop_type(&initializer.ty)?;
                let end = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(end);
                emitter.compiler.end_line_at(global.span, end);
            }
        }
        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::SaveBasePointer));

        if let Some(entry) = emitter.compiler.entry_function {
            if entry.return_type != SemanticType::Void {
                emitter.compiler.emit_stack_alloc(&entry.return_type)?;
            }
            let label = emitter
                .compiler
                .function_labels
                .get(&entry.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(entry.span),
                        format!("missing function label for {:?}", entry.name),
                    )
                })?;
            emitter.compiler.assembler.push_jump(NcsOpcode::Jsr, label);

            if entry.return_type != SemanticType::Void {
                let return_size = size_of_type(&entry.return_type, &emitter.compiler.structs)?;
                // The loader owns the externally visible conditional return
                // slot. Copy the nested entrypoint result down across the
                // globals and saved base-pointer cell before unwinding them.
                let frame_distance = emitter
                    .compiler
                    .global_size
                    .checked_add(4)
                    .and_then(|size| size.checked_add(return_size.saturating_mul(2)))
                    .ok_or_else(|| CodegenError::new(Some(entry.span), "global frame overflow"))?;
                emitter.compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::Assignment,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra:   assignment_extra(
                        -usize_to_i32(frame_distance, "conditional global frame size")?,
                        return_size,
                    )?,
                });
                emitter.compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::ModifyStackPointer,
                    auxcode: NcsAuxCode::None,
                    extra:   (-usize_to_i32(return_size, "conditional return size")?)
                        .to_be_bytes()
                        .to_vec(),
                });
            }
        }

        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::RestoreBasePointer));
        if emitter.compiler.global_size > 0 {
            emitter.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(emitter.compiler.global_size, "global cleanup size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::Ret));
        Ok(())
    }

    pub(super) fn emit_function(&mut self, function: &'a HirFunction) -> Result<(), CodegenError> {
        let previous_function_name = self.current_function_name.replace(function.name.as_str());
        let result = (|| {
            let layout = self.function_layout(function)?;
            let start = self
                .function_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function label for {:?}", function.name),
                    )
                })?;
            let exit = self
                .function_exit_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function exit label for {:?}", function.name),
                    )
                })?;
            let end = self
                .function_end_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function end label for {:?}", function.name),
                    )
                })?;
            if let Some(retval) = &layout.return_layout {
                self.variable_debug.push(VariableDebugInfo {
                    name: "#retval".to_string(),
                    ty: function.return_type.clone(),
                    start,
                    end: Some(end),
                    stack_loc: usize_to_u32(retval.offset, "return stack location")?,
                });
            }
            for parameter in &function.parameters {
                let slot = layout.locals.get(&parameter.local).ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("unknown local slot {:?}", parameter.local),
                    )
                })?;
                self.variable_debug.push(VariableDebugInfo {
                    name: parameter.name.clone(),
                    ty: parameter.ty.clone(),
                    start,
                    end: Some(end),
                    stack_loc: usize_to_u32(slot.offset, "parameter stack location")?,
                });
            }
            let mut emitter = FunctionEmitter {
                compiler: self,
                function,
                layout,
                temp_bytes: 0,
                break_targets: Vec::new(),
                continue_targets: Vec::new(),
                scope_stack: Vec::new(),
            };
            emitter.emit_prologue()?;
            if let Some(body) = &function.body {
                emitter.emit_block(body)?;
                emitter.compiler.assembler.place_label(exit);
                let final_line_start = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(final_line_start);
                emitter
                    .compiler
                    .start_line_end_at(body.span, final_line_start);
                emitter.emit_parameter_cleanup()?;
                emitter
                    .compiler
                    .assembler
                    .push(simple_instruction(NcsOpcode::Ret));
                let final_line_end = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(final_line_end);
                emitter.compiler.end_line_end_at(body.span, final_line_end);
            } else {
                emitter.compiler.assembler.place_label(exit);
                emitter.emit_parameter_cleanup()?;
                emitter
                    .compiler
                    .assembler
                    .push(simple_instruction(NcsOpcode::Ret));
            }
            Ok(())
        })();
        self.current_function_name = previous_function_name;
        result
    }

    pub(super) fn function_layout(
        &self,
        function: &HirFunction,
    ) -> Result<FunctionLayout, CodegenError> {
        let mut offset = 0usize;
        let return_layout = if function.return_type == SemanticType::Void {
            None
        } else {
            let size = size_of_type(&function.return_type, &self.structs)?;
            let layout = ValueLayout {
                offset,
                size,
            };
            offset += size;
            Some(layout)
        };

        let mut locals = BTreeMap::new();
        for parameter in function.parameters.iter().rev() {
            let size = size_of_type(&parameter.ty, &self.structs)?;
            locals.insert(
                parameter.local,
                ValueLayout {
                    offset,
                    size,
                },
            );
            offset += size;
        }

        let return_size = return_layout.as_ref().map_or(0, |layout| layout.size);
        let parameter_size = offset - return_size;
        Ok(FunctionLayout {
            return_layout,
            locals,
            parameter_size,
            active_locals_size: 0,
        })
    }

    pub(super) fn emit_stack_alloc(&mut self, ty: &SemanticType) -> Result<(), CodegenError> {
        match ty {
            SemanticType::Int => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeInteger,
            )),
            SemanticType::Float => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeFloat,
            )),
            SemanticType::String => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeString,
            )),
            SemanticType::Enum {
                name,
                backing,
            } => {
                if let Some(default) = self.hir.enum_defaults.get(name).cloned() {
                    let mut ignored_stack_size = 0;
                    emit_push_literal(self, &mut ignored_stack_size, &default, ty, None)?;
                } else {
                    self.emit_stack_alloc(&enum_backing_semantic_type(*backing))?;
                }
            }
            SemanticType::Object => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeObject,
            )),
            SemanticType::EngineStructure(name) => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                aux_for_engine_structure(name, self.langspec)?,
            )),
            SemanticType::Vector => {
                self.emit_stack_alloc(&SemanticType::Float)?;
                self.emit_stack_alloc(&SemanticType::Float)?;
                self.emit_stack_alloc(&SemanticType::Float)?;
            }
            SemanticType::Struct(name) => {
                let structure = self.structs.get(name).ok_or_else(|| {
                    CodegenError::new(None, format!("unknown structure {name:?}"))
                })?;
                for field in &structure.fields {
                    self.emit_stack_alloc(&field.ty)?;
                }
            }
            SemanticType::Void | SemanticType::Action => {}
        }
        Ok(())
    }

    pub(super) fn start_line_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_location(span) else {
            return;
        };
        self.start_line(source_id, line_num, label);
    }

    pub(super) fn start_line_end_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_end_location(span) else {
            return;
        };
        self.start_line(source_id, line_num, label);
    }

    pub(super) fn start_line(&mut self, source_id: SourceId, line_num: usize, label: LabelId) {
        match &mut self.line_debug.current {
            Some(current) if current.source_id == source_id && current.line_num == line_num => {
                current.refs += 1;
            }
            _ => {
                self.line_debug.current = Some(OpenLineDebug {
                    source_id,
                    line_num,
                    refs: 1,
                    start: label,
                });
            }
        }
    }

    pub(super) fn end_line_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_location(span) else {
            self.line_debug.current = None;
            return;
        };
        self.end_line(source_id, line_num, label);
    }

    pub(super) fn end_line_end_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_end_location(span) else {
            self.line_debug.current = None;
            return;
        };
        self.end_line(source_id, line_num, label);
    }

    pub(super) fn end_line(&mut self, source_id: SourceId, line_num: usize, label: LabelId) {
        let Some(current) = &mut self.line_debug.current else {
            return;
        };
        if current.source_id != source_id || current.line_num != line_num {
            self.line_debug.current = None;
            return;
        }
        if current.refs > 1 {
            current.refs -= 1;
            return;
        }
        let Some(current) = self.line_debug.current.take() else {
            return;
        };
        self.line_debug.entries.push(LineDebugInfo {
            source_id: current.source_id,
            line_num:  current.line_num,
            start:     current.start,
            end:       label,
        });
    }

    pub(super) fn line_location(&self, span: crate::Span) -> Option<(SourceId, usize)> {
        let source_map = self.source_map?;
        let file = source_map.get(span.source_id)?;
        let location = file.location(span.start)?;
        Some((span.source_id, location.line))
    }

    pub(super) fn line_end_location(&self, span: crate::Span) -> Option<(SourceId, usize)> {
        let source_map = self.source_map?;
        let file = source_map.get(span.source_id)?;
        let position = if span.end > span.start {
            span.end - 1
        } else {
            span.start
        };
        let location = file.location(position)?;
        Some((span.source_id, location.line))
    }

    pub(super) fn magic_literal_value(
        &self,
        literal: crate::MagicLiteral,
        span: Option<crate::Span>,
    ) -> Result<Literal, CodegenError> {
        Ok(match literal {
            crate::MagicLiteral::Function => Literal::String(ScriptString::new(
                self.current_function_name
                    .unwrap_or_default()
                    .as_bytes()
                    .to_vec(),
            )),
            crate::MagicLiteral::File => {
                let mut value = span
                    .and_then(|span| self.source_map?.get(span.source_id))
                    .map(|file| file.name.clone())
                    .unwrap_or_default();
                if !value.is_empty() && Path::new(&value).extension().is_none() {
                    value.push_str(".nss");
                }
                Literal::String(ScriptString::try_from_text(&value).map_err(|error| {
                    CodegenError::new(
                        span,
                        format!("__FILE__ is not representable in Windows-1252: {error}"),
                    )
                })?)
            }
            crate::MagicLiteral::Line => {
                let value = span
                    .and_then(|span| self.line_location(span))
                    .map_or(0, |(_source_id, line)| {
                        i32::try_from(line).ok().unwrap_or(i32::MAX)
                    });
                Literal::Integer(value)
            }
            crate::MagicLiteral::Date => Literal::String(ScriptString::new(
                format_magic_date(self.compile_time).into_bytes(),
            )),
            crate::MagicLiteral::Time => Literal::String(ScriptString::new(
                format_magic_time(self.compile_time).into_bytes(),
            )),
        })
    }
}
