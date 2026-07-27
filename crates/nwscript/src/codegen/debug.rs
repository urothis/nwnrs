use std::collections::BTreeMap;

use super::{CodegenError, CodegenOutput, LabelId, engine_structure_index};
use crate::{
    CancellationToken, HirModule, LangSpec, NCS_BINARY_HEADER_SIZE, Ndb, NdbFile, NdbFunction,
    NdbLine, NdbStruct, NdbStructField, NdbType, NdbVariable, SemanticType, SourceId, SourceMap,
};

pub(super) fn build_ndb(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    source_map: &SourceMap,
    root_id: SourceId,
    output: &CodegenOutput,
    cancellation: Option<&CancellationToken>,
) -> Result<Ndb, CodegenError> {
    let mut lines = Vec::new();
    let mut file_order = Vec::new();
    let mut file_indices = BTreeMap::new();

    for line in &output.lines {
        check_codegen_cancellation(cancellation)?;
        let file = source_map.get(line.source_id).ok_or_else(|| {
            CodegenError::new(
                None,
                format!(
                    "debug line references missing source file {}",
                    line.source_id.get()
                ),
            )
        })?;
        let file_num = if let Some(file_num) = file_indices.get(&file.id).copied() {
            file_num
        } else {
            let file_num = file_order.len();
            file_order.push(file.id);
            file_indices.insert(file.id, file_num);
            file_num
        };
        lines.push(NdbLine {
            file_num,
            line_num: line.line_num,
            binary_start: debug_label_offset(output, line.start, "source-line start", None)?,
            binary_end: debug_label_offset(output, line.end, "source-line end", None)?,
        });
    }

    if let std::collections::btree_map::Entry::Vacant(entry) = file_indices.entry(root_id) {
        let root = source_map
            .get(root_id)
            .ok_or_else(|| CodegenError::new(None, "debug root source file is missing"))?;
        entry.insert(file_order.len());
        file_order.push(root.id);
    }

    let mut files = Vec::with_capacity(file_order.len());
    for file_id in file_order {
        check_codegen_cancellation(cancellation)?;
        let file = source_map.get(file_id).ok_or_else(|| {
            CodegenError::new(
                None,
                format!(
                    "debug file table references missing source {}",
                    file_id.get()
                ),
            )
        })?;
        files.push(NdbFile {
            name:    file.name.clone(),
            is_root: file.id == root_id,
        });
    }

    let structs = hir
        .structs
        .iter()
        .map(|structure| {
            check_codegen_cancellation(cancellation)?;
            Ok::<_, CodegenError>(NdbStruct {
                label:  structure.name.clone(),
                fields: structure
                    .fields
                    .iter()
                    .map(|field| {
                        check_codegen_cancellation(cancellation)?;
                        Ok(NdbStructField {
                            label: field.name.clone(),
                            ty:    debug_type_for_semantic(&field.ty, hir, langspec)?,
                        })
                    })
                    .collect::<Result<Vec<_>, CodegenError>>()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let functions = hir
        .functions
        .iter()
        .filter(|function| !function.is_builtin)
        .map(|function| {
            check_codegen_cancellation(cancellation)?;
            let info = output.functions.get(&function.name).ok_or_else(|| {
                CodegenError::new(
                    Some(function.span),
                    format!("missing debug range for {:?}", function.name),
                )
            })?;
            Ok::<_, CodegenError>(NdbFunction {
                label:        function.name.clone(),
                binary_start: debug_label_offset(
                    output,
                    info.start,
                    "function start",
                    Some(function.span),
                )?,
                binary_end:   debug_label_offset(
                    output,
                    info.end,
                    "function end",
                    Some(function.span),
                )?,
                return_type:  debug_type_for_semantic(&function.return_type, hir, langspec)?,
                args:         function
                    .parameters
                    .iter()
                    .map(|parameter| {
                        check_codegen_cancellation(cancellation)?;
                        debug_type_for_semantic(&parameter.ty, hir, langspec)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let variables = output
        .variables
        .iter()
        .map(|variable| {
            check_codegen_cancellation(cancellation)?;
            Ok::<_, CodegenError>(NdbVariable {
                label:        variable.name.clone(),
                ty:           debug_type_for_semantic(&variable.ty, hir, langspec)?,
                binary_start: debug_label_offset(output, variable.start, "variable start", None)?,
                binary_end:   match variable.end {
                    Some(end) => debug_label_offset(output, end, "variable end", None)?,
                    None => u32::MAX,
                },
                stack_loc:    variable.stack_loc,
            })
        })
        .collect::<Result<Vec<_>, CodegenError>>()?;

    Ok(Ndb {
        files,
        structs,
        functions,
        variables,
        lines,
    })
}

fn check_codegen_cancellation(
    cancellation: Option<&CancellationToken>,
) -> Result<(), CodegenError> {
    if let Some(cancellation) = cancellation {
        cancellation
            .check()
            .map_err(|error| CodegenError::new(None, error.to_string()))?;
    }
    Ok(())
}

fn debug_label_offset(
    output: &CodegenOutput,
    label: LabelId,
    description: &str,
    span: Option<crate::Span>,
) -> Result<u32, CodegenError> {
    let code_offset = output.label_offsets.get(&label).copied().ok_or_else(|| {
        CodegenError::new(
            span,
            format!("missing debug label offset for {description}"),
        )
    })?;
    debug_binary_offset(code_offset, description, span)
}

fn debug_binary_offset(
    code_offset: u32,
    description: &str,
    span: Option<crate::Span>,
) -> Result<u32, CodegenError> {
    const NCS_HEADER_BYTES: u32 = 13;
    debug_assert_eq!(NCS_BINARY_HEADER_SIZE, 13);
    code_offset.checked_add(NCS_HEADER_BYTES).ok_or_else(|| {
        CodegenError::new(
            span,
            format!("{description} debug offset exceeds the NCS address range"),
        )
    })
}

fn debug_type_for_semantic(
    ty: &SemanticType,
    hir: &HirModule,
    langspec: Option<&LangSpec>,
) -> Result<NdbType, CodegenError> {
    Ok(match ty {
        SemanticType::Float => NdbType::Float,
        SemanticType::Int => NdbType::Int,
        SemanticType::Void => NdbType::Void,
        SemanticType::Object => NdbType::Object,
        SemanticType::String => NdbType::String,
        SemanticType::EngineStructure(name) => {
            NdbType::EngineStructure(engine_structure_index(name, langspec)?)
        }
        SemanticType::Struct(name) => {
            let index = hir
                .structs
                .iter()
                .position(|structure| structure.name == *name)
                .ok_or_else(|| {
                    CodegenError::new(None, format!("unknown debug structure {name:?}"))
                })?;
            NdbType::Struct(index)
        }
        SemanticType::Vector | SemanticType::Action => NdbType::Unknown,
        SemanticType::Enum {
            backing: crate::EnumBackingType::Int,
            ..
        } => NdbType::Int,
        SemanticType::Enum {
            backing: crate::EnumBackingType::String,
            ..
        } => NdbType::String,
    })
}
