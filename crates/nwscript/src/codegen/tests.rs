use std::collections::BTreeMap;

use super::{
    CompileError, CompileOptions, MAX_COMPILER_IDENTIFIERS, MAX_COMPILER_RUNTIME_CELLS,
    OptimizationFlag, OptimizationFlags, validate_hir_limits,
};
use crate::{
    BuiltinConstant, BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue, HirBlock,
    HirFunction, HirLocal, HirLocalId, HirLocalKind, HirModule, NcsAuxCode, NcsOpcode,
    SemanticType, SourceId, SourceMap, Span, compile_script, compile_script_with_source_map,
    compile_source_bundle, decode_ncs_instructions, load_source_bundle, parse_text, read_ndb,
};

fn empty_hir_with_locals(local_count: usize) -> HirModule {
    let span = Span::new(SourceId::new(0), 0, 0);
    HirModule {
        includes:      Vec::new(),
        structs:       Vec::new(),
        enum_defaults: BTreeMap::new(),
        globals:       Vec::new(),
        functions:     vec![HirFunction {
            span,
            name: "main".to_string(),
            return_type: SemanticType::Void,
            parameters: Vec::new(),
            locals: (0..local_count)
                .map(|index| HirLocal {
                    id:               HirLocalId(
                        u32::try_from(index).expect("test local index fits u32"),
                    ),
                    declaration_span: span,
                    name:             format!("local_{index}"),
                    ty:               SemanticType::Int,
                    kind:             HirLocalKind::Local,
                })
                .collect(),
            body: Some(HirBlock {
                span,
                statements: Vec::new(),
            }),
            is_builtin: false,
        }],
    }
}

#[test]
fn reports_native_script_size_limit() {
    let hir = empty_hir_with_locals(MAX_COMPILER_RUNTIME_CELLS + 1);
    let error = validate_hir_limits(&hir, None).expect_err("oversized frame should fail");
    assert_eq!(error.code, Some(crate::CompilerErrorCode::ScriptTooLarge));
}

#[test]
fn reports_native_identifier_table_limit() {
    let mut langspec = test_langspec();
    let prototype = langspec
        .constants
        .first()
        .cloned()
        .expect("test langspec contains a constant");
    langspec.constants = vec![prototype; MAX_COMPILER_IDENTIFIERS];

    let error = validate_hir_limits(&empty_hir_with_locals(0), Some(&langspec))
        .expect_err("full identifier table should fail");
    assert_eq!(
        error.code,
        Some(crate::CompilerErrorCode::IdentifierListFull)
    );
}

#[test]
fn optimization_passes_are_independently_selectable() {
    let flags = OptimizationFlag::RemoveDeadBranches | OptimizationFlag::MeldInstructions;

    assert!(flags.contains(OptimizationFlag::RemoveDeadBranches));
    assert!(flags.contains(OptimizationFlag::MeldInstructions));
    assert!(!flags.contains(OptimizationFlag::RemoveDeadCode));
    assert_eq!(flags.level(), None);
    assert_eq!(OptimizationFlags::from_bits(flags.bits()), Some(flags));
    assert_eq!(OptimizationFlags::from_bits(0x80), None);

    let mut incremental = OptimizationFlags::O0;
    incremental |= OptimizationFlag::RemoveDeadCode;
    assert_eq!(incremental, OptimizationFlags::O1);
}

#[test]
fn source_bundle_parse_failures_preserve_the_parse_error_type() {
    let mut resolver = crate::InMemoryScriptResolver::new();
    resolver.insert_source("broken", "void main( {");
    let bundle = load_source_bundle(&resolver, "broken", crate::SourceLoadOptions::default())
        .expect("source should load before parsing");

    let error = compile_source_bundle(&bundle, None, CompileOptions::default())
        .expect_err("malformed source should fail parsing");
    assert!(matches!(error, CompileError::Parse(_)));
}

fn decode_string_constant(extra: &[u8]) -> String {
    String::from_utf8(decode_string_constant_bytes(extra)).expect("string constant should be utf-8")
}

fn decode_string_constant_bytes(extra: &[u8]) -> Vec<u8> {
    let length_bytes: [u8; 2] = extra
        .get(..2)
        .expect("string constant should include a 16-bit length prefix")
        .try_into()
        .expect("string constant length prefix should be two bytes");
    let length = u16::from_be_bytes(length_bytes) as usize;
    let payload = extra
        .get(2..2 + length)
        .expect("string constant payload should match its encoded length");
    payload.to_vec()
}

fn decode_integer_constant(extra: &[u8]) -> i32 {
    let bytes: [u8; 4] = extra
        .get(..4)
        .expect("integer constant should encode four bytes")
        .try_into()
        .expect("integer constant prefix should be four bytes");
    i32::from_be_bytes(bytes)
}

fn test_langspec() -> crate::LangSpec {
    crate::LangSpec {
        engine_num_structures: 3,
        engine_structures:     vec![
            "effect".to_string(),
            "location".to_string(),
            "json".to_string(),
        ],
        constants:             vec![
            BuiltinConstant {
                name:  "TRUE".to_string(),
                ty:    BuiltinType::Int,
                value: BuiltinValue::Int(1),
            },
            BuiltinConstant {
                name:  "FALSE".to_string(),
                ty:    BuiltinType::Int,
                value: BuiltinValue::Int(0),
            },
            BuiltinConstant {
                name:  "OBJECT_SELF".to_string(),
                ty:    BuiltinType::Object,
                value: BuiltinValue::ObjectSelf,
            },
            BuiltinConstant {
                name:  "OBJECT_INVALID".to_string(),
                ty:    BuiltinType::Object,
                value: BuiltinValue::ObjectInvalid,
            },
            BuiltinConstant {
                name:  "OBJECT_TYPE_CREATURE".to_string(),
                ty:    BuiltinType::Int,
                value: BuiltinValue::Int(1),
            },
        ],
        functions:             vec![
            BuiltinFunction {
                name:        "GetCurrentHitPoints".to_string(),
                return_type: BuiltinType::Int,
                parameters:  vec![],
            },
            BuiltinFunction {
                name:        "GetMaxHitPoints".to_string(),
                return_type: BuiltinType::Int,
                parameters:  vec![],
            },
            BuiltinFunction {
                name:        "CreateObject".to_string(),
                return_type: BuiltinType::Object,
                parameters:  vec![
                    BuiltinParameter {
                        name:    "nObjectType".to_string(),
                        ty:      BuiltinType::Int,
                        default: None,
                    },
                    BuiltinParameter {
                        name:    "sTemplate".to_string(),
                        ty:      BuiltinType::String,
                        default: None,
                    },
                    BuiltinParameter {
                        name:    "lLocation".to_string(),
                        ty:      BuiltinType::EngineStructure("location".to_string()),
                        default: None,
                    },
                ],
            },
            BuiltinFunction {
                name:        "GetLocation".to_string(),
                return_type: BuiltinType::EngineStructure("location".to_string()),
                parameters:  vec![BuiltinParameter {
                    name:    "oTarget".to_string(),
                    ty:      BuiltinType::Object,
                    default: None,
                }],
            },
            BuiltinFunction {
                name:        "SetListening".to_string(),
                return_type: BuiltinType::Void,
                parameters:  vec![
                    BuiltinParameter {
                        name:    "oTarget".to_string(),
                        ty:      BuiltinType::Object,
                        default: None,
                    },
                    BuiltinParameter {
                        name:    "bValue".to_string(),
                        ty:      BuiltinType::Int,
                        default: None,
                    },
                ],
            },
            BuiltinFunction {
                name:        "SetListenPattern".to_string(),
                return_type: BuiltinType::Void,
                parameters:  vec![
                    BuiltinParameter {
                        name:    "oTarget".to_string(),
                        ty:      BuiltinType::Object,
                        default: None,
                    },
                    BuiltinParameter {
                        name:    "sPattern".to_string(),
                        ty:      BuiltinType::String,
                        default: None,
                    },
                    BuiltinParameter {
                        name:    "nNumber".to_string(),
                        ty:      BuiltinType::Int,
                        default: None,
                    },
                ],
            },
            BuiltinFunction {
                name:        "SpeakString".to_string(),
                return_type: BuiltinType::Void,
                parameters:  vec![BuiltinParameter {
                    name:    "sMessage".to_string(),
                    ty:      BuiltinType::String,
                    default: None,
                }],
            },
            BuiltinFunction {
                name:        "DelayCommand".to_string(),
                return_type: BuiltinType::Void,
                parameters:  vec![
                    BuiltinParameter {
                        name:    "fSeconds".to_string(),
                        ty:      BuiltinType::Float,
                        default: None,
                    },
                    BuiltinParameter {
                        name:    "aAction".to_string(),
                        ty:      BuiltinType::Action,
                        default: Some(BuiltinValue::Raw(
                            "SpeakString(\"default action\")".to_string(),
                        )),
                    },
                ],
            },
        ],
    }
}

#[test]
fn compiles_conditional_script_to_valid_ncs() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(80),
        r#"
            int StartingConditional() {
                int nCurHP = GetCurrentHitPoints();
                int nMaxHP = GetMaxHitPoints();
                if (nCurHP < (nMaxHP / 4)) {
                    return TRUE;
                }
                return FALSE;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(
        &script,
        Some(&test_langspec()),
        CompileOptions {
            optimizations: OptimizationFlags::O0,
            ..CompileOptions::default()
        },
    )?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(!instructions.is_empty());
    assert_eq!(
        instructions.first().map(|instruction| instruction.opcode),
        Some(NcsOpcode::RunstackAdd)
    );
    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Jsr)
    );
    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Ret)
    );
    Ok(())
}

#[test]
fn compiles_builtin_action_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(95),
        r#"
            void main() {
                DelayCommand(1.0);
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::StoreState),
        "builtin action defaults should emit an embedded action body"
    );
    assert!(
        instructions.iter().any(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == NcsAuxCode::TypeString
                && decode_string_constant(&instruction.extra) == "default action"
        }),
        "builtin action default body should be compiled from its raw langspec expression"
    );

    Ok(())
}

#[test]
fn compiles_simple_user_and_builtin_calls_to_valid_ncs() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(81),
        r#"
            void SetupListening(object oCheater) {
                SetListening(oCheater, TRUE);
                SetListenPattern(oCheater, "1", 1001);
            }
            void main() {
                object oCheater = CreateObject(OBJECT_TYPE_CREATURE, "x0_cheater", GetLocation(OBJECT_SELF));
                SetupListening(oCheater);
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::ExecuteCommand)
    );
    assert!(
        instructions
            .iter()
            .filter(|instruction| instruction.opcode == NcsOpcode::Jsr)
            .count()
            >= 2
    );
    Ok(())
}

#[test]
fn compiles_strong_int_and_string_enums_to_native_storage() -> Result<(), Box<dyn std::error::Error>>
{
    let script = parse_text(
        SourceId::new(181),
        r#"
            enum LogLevel {
                Trace,
                #[default] #[alias(LOG_INFO)] Info = 2,
            }
            enum EventPhase : string {
                #[alias(EVENT_BEFORE)] Before = "before",
                After = "after",
            }
            type Level = LogLevel;
            static_assert(int(LogLevel::Info) == 2, "bad enum value");

            int raw_level(Level level) { return int(level); }
            string raw_phase(EventPhase phase) { return string(phase); }

            void main() {
                Level level;
                EventPhase phase = EventPhase::After;
                int raw = raw_level(LOG_INFO);
                string text = raw_phase(phase);
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;
    assert!(instructions.iter().any(|instruction| {
        instruction.opcode == NcsOpcode::Constant
            && instruction.auxcode == NcsAuxCode::TypeInteger
            && instruction.extra == 2_i32.to_be_bytes().to_vec()
    }));
    assert!(instructions.iter().any(|instruction| {
        instruction.opcode == NcsOpcode::Constant && instruction.auxcode == NcsAuxCode::TypeString
    }));
    Ok(())
}

#[test]
fn compiles_windows_1252_strings_to_exact_ncs_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(182),
        r#"
            void main() {
                string value = "café\x81<c\x01\x80\xFF>x</c>";
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;
    let expected = b"caf\xe9\x81<c\x01\x80\xff>x</c>";
    assert!(instructions.iter().any(|instruction| {
        instruction.opcode == NcsOpcode::Constant
            && instruction.auxcode == NcsAuxCode::TypeString
            && decode_string_constant_bytes(&instruction.extra) == expected
    }));
    Ok(())
}

#[test]
fn compiles_user_defined_optional_parameter_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(82),
        r#"
            int AddOne(int nBase = TRUE) {
                return nBase + 1;
            }
            int StartingConditional() {
                return AddOne();
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Jsr)
    );
    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == crate::NcsAuxCode::TypeInteger
                && instruction.extra == 1_i32.to_be_bytes().to_vec()),
        "default integer argument should be materialized before the user call"
    );
    Ok(())
}

#[test]
fn compiles_calls_using_defaults_from_forward_declarations()
-> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(84),
        r#"
            void helper(object oTarget = OBJECT_INVALID);
            void helper(object oTarget) {}
            void main() {
                helper();
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Jsr)
    );
    Ok(())
}

#[test]
fn non_void_returns_jump_to_a_shared_function_exit() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(85),
        r#"
            int StartingConditional() {
                if (TRUE) {
                    return TRUE;
                }
                return FALSE;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| instruction.opcode == NcsOpcode::Ret)
            .count(),
        2,
        "the module should only emit one loader RET and one shared function-exit RET",
    );
    assert!(
        instructions
            .iter()
            .filter(|instruction| instruction.opcode == NcsOpcode::Jmp)
            .count()
            >= 2,
        "non-void return statements should branch to the shared function exit",
    );
    Ok(())
}

#[test]
fn ndb_includes_upstream_debugger_synthetic_retvals() {
    let source = br#"
        int StartingConditional() {
            return TRUE;
        }
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("synthetic_retval.nss".to_string(), source.to_vec());
    let script = parse_text(
        root_id,
        std::str::from_utf8(source).expect("utf-8"),
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&test_langspec()),
        CompileOptions::default(),
    )
    .expect("compile should succeed");
    let ndb = read_ndb(&mut std::io::Cursor::new(
        artifacts.ndb.expect("NDB output should be present"),
    ))
    .expect("NDB should parse");

    assert!(
        ndb.variables
            .iter()
            .any(|variable| variable.label == "#retval"),
        "non-void entrypoint should preserve #retval debug records",
    );
    let loader_retval = ndb
        .variables
        .iter()
        .find(|variable| variable.label == "#retval" && variable.binary_end == u32::MAX)
        .expect("loader #retval debug record should be present");
    assert_eq!(
        loader_retval.binary_start, 15,
        "loader #retval should begin after the loader RunstackAdd instruction",
    );
}

#[test]
fn optimized_compiles_preserve_coherent_ndb_ranges() {
    let source = br#"
        int Unused() { return 9; }
        void main() {
            int nValue = 1;
            if (FALSE) { nValue = 2; }
        }
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("optimized_debug.nss".to_string(), source.to_vec());
    let script = parse_text(
        root_id,
        std::str::from_utf8(source).expect("utf-8"),
        Some(&test_langspec()),
    )
    .expect("script should parse");

    for optimizations in [
        OptimizationFlags::O1,
        OptimizationFlags::O2,
        OptimizationFlags::O3,
    ] {
        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&test_langspec()),
            CompileOptions {
                optimizations,
                ..CompileOptions::default()
            },
        )
        .expect("optimized compile should succeed");
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts
                .ndb
                .expect("optimized NDB output should be present"),
        ))
        .expect("optimized NDB should parse");
        let code_size = u32::try_from(artifacts.ncs.len()).expect("test NCS should fit in u32");

        assert!(
            ndb.functions
                .iter()
                .any(|function| function.label == "main")
        );
        assert!(
            !ndb.functions
                .iter()
                .any(|function| function.label == "Unused")
        );
        assert!(ndb.functions.iter().all(|function| {
            function.binary_start <= function.binary_end && function.binary_end <= code_size
        }));
        assert!(
            ndb.lines.iter().all(|line| {
                line.binary_start <= line.binary_end && line.binary_end <= code_size
            })
        );
    }
}

#[test]
fn ndb_tracks_switch_eval_and_block_local_lifetimes() {
    let source = br#"
        void main() {
            switch (TRUE) {
                case TRUE:
                {
                    int nLocal = 1;
                    break;
                }
                default:
                    break;
            }
        }
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("switch_debug.nss".to_string(), source.to_vec());
    let script = parse_text(
        root_id,
        std::str::from_utf8(source).expect("utf-8"),
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&test_langspec()),
        CompileOptions::default(),
    )
    .expect("compile should succeed");
    let ndb = read_ndb(&mut std::io::Cursor::new(
        artifacts.ndb.expect("NDB output should be present"),
    ))
    .expect("NDB should parse");

    let switch_eval = ndb
        .variables
        .iter()
        .find(|variable| variable.label == "#switcheval")
        .expect("switch debug variable should be present");
    let local = ndb
        .variables
        .iter()
        .find(|variable| variable.label == "nLocal")
        .expect("block local should be present");

    assert!(
        switch_eval.binary_end >= local.binary_end,
        "switch eval lifetime should cover the switch body",
    );
    assert!(
        local.binary_start > switch_eval.binary_start,
        "block local should start after switch evaluation begins",
    );
}

#[test]
fn ndb_global_debug_starts_after_global_allocations() {
    let source = br#"
        int FIRST = TRUE;
        int SECOND = FALSE;
        void main() {}
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("globals_debug.nss".to_string(), source.to_vec());
    let script = parse_text(
        root_id,
        std::str::from_utf8(source).expect("utf-8"),
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&test_langspec()),
        CompileOptions::default(),
    )
    .expect("compile should succeed");
    let ndb = read_ndb(&mut std::io::Cursor::new(
        artifacts.ndb.expect("NDB output should be present"),
    ))
    .expect("NDB should parse");

    let first = ndb
        .variables
        .iter()
        .find(|variable| variable.label == "FIRST")
        .expect("FIRST global should be present");
    let second = ndb
        .variables
        .iter()
        .find(|variable| variable.label == "SECOND")
        .expect("SECOND global should be present");

    assert_eq!(
        first.binary_start, 23,
        "first global should begin after loader + first RunstackAdd"
    );
    assert_eq!(
        second.binary_start, 45,
        "second global should begin after the first initializer + second RunstackAdd"
    );
    assert_eq!(first.stack_loc, 0);
    assert_eq!(second.stack_loc, 4);
}

#[test]
fn compiles_switch_cases_backed_by_const_globals() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(84),
        r#"
            const int CASE_A = 1 + 2;
            int StartingConditional() {
                int nValue = 3;
                switch (nValue) {
                    case CASE_A:
                        return TRUE;
                    default:
                        return FALSE;
                }
                return FALSE;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Equal),
        "switch codegen should materialize a case comparison",
    );
    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Jmp),
        "switch codegen should branch into case bodies",
    );
    Ok(())
}

#[test]
fn compiles_magic_literals_with_source_context() -> Result<(), Box<dyn std::error::Error>> {
    let source = br#"void main() {
string sFunction = __FUNCTION__;
string sFile = __FILE__;
int nLine = __LINE__;
}
"#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("magic_literals.nss".to_string(), source.to_vec());
    let script = parse_text(
        root_id,
        std::str::from_utf8(source).expect("utf-8"),
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&test_langspec()),
        CompileOptions::default(),
    )?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    let string_constants = instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == crate::NcsAuxCode::TypeString
        })
        .map(|instruction| decode_string_constant(&instruction.extra))
        .collect::<Vec<_>>();
    let integer_constants = instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == crate::NcsAuxCode::TypeInteger
        })
        .map(|instruction| decode_integer_constant(&instruction.extra))
        .collect::<Vec<_>>();

    assert!(string_constants.iter().any(|value| value == "main"));
    assert!(
        string_constants
            .iter()
            .any(|value| value == "magic_literals.nss")
    );
    assert!(integer_constants.contains(&4));
    Ok(())
}

#[test]
fn file_magic_adds_nss_to_extensionless_include_names() -> Result<(), Box<dyn std::error::Error>> {
    let source = br#"void Included() { string file = __FILE__; } void main() {}"#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("inc_helpers".to_string(), source.to_vec());
    let script = parse_text(
        root_id,
        std::str::from_utf8(source)?,
        Some(&test_langspec()),
    )?;
    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&test_langspec()),
        CompileOptions::default(),
    )?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(instructions.iter().any(|instruction| {
        instruction.opcode == NcsOpcode::Constant
            && instruction.auxcode == NcsAuxCode::TypeString
            && decode_string_constant(&instruction.extra) == "inc_helpers.nss"
    }));
    Ok(())
}

#[test]
fn compiles_magic_literals_without_source_map() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(86),
        r#"
            void main() {
                string sFunction = __FUNCTION__;
                string sFile = __FILE__;
                int nLine = __LINE__;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    let string_constants = instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == crate::NcsAuxCode::TypeString
        })
        .map(|instruction| decode_string_constant(&instruction.extra))
        .collect::<Vec<_>>();
    let integer_constants = instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == crate::NcsAuxCode::TypeInteger
        })
        .map(|instruction| decode_integer_constant(&instruction.extra))
        .collect::<Vec<_>>();

    assert!(string_constants.iter().any(|value| value == "main"));
    assert!(string_constants.iter().any(|value| value.is_empty()));
    assert!(integer_constants.contains(&0));
    Ok(())
}

#[test]
fn compiles_date_and_time_magic_literals() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(87),
        r#"
            void main() {
                string sDate = __DATE__;
                string sTime = __TIME__;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;
    let string_constants = instructions
        .iter()
        .filter(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == crate::NcsAuxCode::TypeString
        })
        .map(|instruction| decode_string_constant(&instruction.extra))
        .collect::<Vec<_>>();

    assert!(
        string_constants.iter().any(|value| {
            let bytes = value.as_bytes();
            value.len() == 10
                && bytes.get(4) == Some(&b'-')
                && bytes.get(7) == Some(&b'-')
                && value
                    .chars()
                    .enumerate()
                    .all(|(index, ch)| matches!(index, 4 | 7) || ch.is_ascii_digit())
        }),
        "__DATE__ should compile into an upstream-compatible YYYY-MM-DD string",
    );
    assert!(
        string_constants.iter().any(|value| {
            let bytes = value.as_bytes();
            value.len() == 8
                && bytes.get(2) == Some(&b':')
                && bytes.get(5) == Some(&b':')
                && value
                    .chars()
                    .enumerate()
                    .all(|(index, ch)| matches!(index, 2 | 5) || ch.is_ascii_digit())
        }),
        "__TIME__ should compile into a macro-style time string",
    );
    Ok(())
}

#[test]
fn compiles_conditional_expressions() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(88),
        r#"
            int StartingConditional() {
                int nCurHP = GetCurrentHitPoints();
                return nCurHP > 0 ? TRUE : FALSE;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Jz),
        "conditional expression should branch on the computed condition",
    );
    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Jmp),
        "conditional expression should merge control flow after one arm executes",
    );
    Ok(())
}

#[test]
fn compiles_nested_field_assignments() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(89),
        r#"
            struct Inner { int value; };
            struct Outer { struct Inner inner; };
            void main() {
                struct Outer outer;
                outer.inner.value = 1;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Assignment),
        "nested field assignment should lower to an assignment opcode",
    );
    Ok(())
}

#[test]
fn emits_native_short_circuit_and_switch_branches() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(90),
        r#"
            int StartingConditional() {
                int n = GetCurrentHitPoints();
                switch (n) {
                    case 1:
                        return n == 1 || GetMaxHitPoints() > 0;
                    default:
                        return FALSE;
                }
                return FALSE;
            }
        "#,
        Some(&test_langspec()),
    )?;
    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(instructions.iter().any(|instruction| {
        instruction.opcode == NcsOpcode::LogicalOr
            && instruction.auxcode == NcsAuxCode::TypeTypeIntegerInteger
    }));
    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::Jnz),
        "switch cases should branch directly with JNZ",
    );
    Ok(())
}

#[test]
fn emits_native_direct_increment_opcodes() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(91),
        "void main() { int n = 0; n++; ++n; }",
        Some(&test_langspec()),
    )?;
    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| instruction.opcode == NcsOpcode::Increment)
            .count(),
        2,
    );
    let increment_offsets = instructions
        .iter()
        .filter(|instruction| instruction.opcode == NcsOpcode::Increment)
        .map(|instruction| {
            i32::from_be_bytes(
                instruction
                    .extra
                    .get(..4)
                    .expect("increment offset should be four bytes")
                    .try_into()
                    .expect("increment offset should be four bytes"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(increment_offsets, vec![-8, -4]);
    assert!(!instructions.iter().any(|instruction| {
        instruction.opcode == NcsOpcode::Add
            && instruction.auxcode == NcsAuxCode::TypeTypeIntegerInteger
    }));
    Ok(())
}

#[test]
fn const_globals_do_not_create_a_runtime_global_frame() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(92),
        "const int VALUE = 7; int StartingConditional() { return VALUE; }",
        Some(&test_langspec()),
    )?;
    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        !instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::SaveBasePointer),
    );
    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| {
                instruction.opcode == NcsOpcode::RunstackAdd
                    && instruction.auxcode == NcsAuxCode::TypeInteger
            })
            .count(),
        1,
        "only the loader conditional return slot should be allocated",
    );
    assert!(instructions.iter().any(|instruction| {
        instruction.opcode == NcsOpcode::Constant
            && instruction.auxcode == NcsAuxCode::TypeInteger
            && decode_integer_constant(&instruction.extra) == 7
    }));
    Ok(())
}

#[test]
fn top_level_struct_retains_native_global_wrapper() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(93),
        "struct Pair { int value; }; void main() {}",
        Some(&test_langspec()),
    )?;
    let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
    let instructions = decode_ncs_instructions(&artifacts.ncs)?;

    assert!(
        instructions
            .iter()
            .any(|instruction| instruction.opcode == NcsOpcode::SaveBasePointer),
    );
    Ok(())
}
