use super::{SemanticType, analyze_script, analyze_script_with_options};
use crate::{LangSpec, Literal, SemanticOptions, SourceId, parse_text};

fn test_langspec() -> LangSpec {
    LangSpec {
        engine_num_structures: 3,
        engine_structures:     vec![
            "effect".to_string(),
            "location".to_string(),
            "json".to_string(),
        ],
        constants:             vec![
            crate::BuiltinConstant {
                name:  "TRUE".to_string(),
                ty:    crate::BuiltinType::Int,
                value: crate::BuiltinValue::Int(1),
            },
            crate::BuiltinConstant {
                name:  "FALSE".to_string(),
                ty:    crate::BuiltinType::Int,
                value: crate::BuiltinValue::Int(0),
            },
            crate::BuiltinConstant {
                name:  "OBJECT_INVALID".to_string(),
                ty:    crate::BuiltinType::Object,
                value: crate::BuiltinValue::ObjectInvalid,
            },
        ],
        functions:             vec![
            crate::BuiltinFunction {
                name:        "DelayCommand".to_string(),
                return_type: crate::BuiltinType::Void,
                parameters:  vec![
                    crate::BuiltinParameter {
                        name:    "fSeconds".to_string(),
                        ty:      crate::BuiltinType::Float,
                        default: None,
                    },
                    crate::BuiltinParameter {
                        name:    "aAction".to_string(),
                        ty:      crate::BuiltinType::Action,
                        default: None,
                    },
                ],
            },
            crate::BuiltinFunction {
                name:        "EffectDamage".to_string(),
                return_type: crate::BuiltinType::EngineStructure("effect".to_string()),
                parameters:  vec![crate::BuiltinParameter {
                    name:    "nAmount".to_string(),
                    ty:      crate::BuiltinType::Int,
                    default: None,
                }],
            },
        ],
    }
}

#[test]
fn resolves_functions_globals_and_structs() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(40),
        "struct Foo { int value; }; effect gFx; void helper(int n = TRUE); void helper(int n = \
         TRUE) { int x = n; } void main() { struct Foo f; int x = f.value; }",
        Some(&test_langspec()),
    )?;

    let model = analyze_script(&script, Some(&test_langspec()))?;
    assert!(model.structs.contains_key("Foo"));
    assert!(model.functions.contains_key("helper"));
    assert!(model.functions.contains_key("EffectDamage"));
    assert_eq!(
        model
            .globals
            .get("gFx")
            .ok_or_else(|| std::io::Error::other("missing global gFx"))?
            .ty,
        SemanticType::EngineStructure("effect".to_string())
    );
    Ok(())
}

#[test]
fn resolves_strong_int_and_string_enums_aliases_and_static_assertions()
-> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(140),
        r#"
            enum LogLevel {
                Trace,
                #[default] Debug = Trace + 2,
                #[alias(LOG_INFO)] Info,
            }
            enum EventPhase : string {
                Before = "before",
                #[alias(EVENT_AFTER)] After = "after",
            }
            type Level = LogLevel;
            static_assert(LogLevel::Trace != LogLevel::Debug, "levels must differ");
            static_assert(Level::Trace == LogLevel::Trace, "aliases qualify variants");
            void take_level(Level level);
            void take_phase(EventPhase phase);
            void main() {
                Level level = Level::Debug;
                EventPhase phase = EventPhase::Before;
                take_level(LOG_INFO);
                take_phase(EVENT_AFTER);
                static_assert(LogLevel::Info == LOG_INFO);
            }
        "#,
        Some(&test_langspec()),
    )?;

    let model = analyze_script(&script, Some(&test_langspec()))?;
    let log_level = model.enums.get("LogLevel").ok_or("missing LogLevel")?;
    assert_eq!(log_level.default_variant, 1);
    assert_eq!(log_level.variants.len(), 3);
    assert_eq!(
        model.aliases.get("Level"),
        Some(&SemanticType::Enum {
            name:    "LogLevel".to_string(),
            backing: crate::EnumBackingType::Int,
        })
    );
    assert_eq!(
        model.globals.get("LOG_INFO").map(|global| &global.ty),
        Some(&SemanticType::Enum {
            name:    "LogLevel".to_string(),
            backing: crate::EnumBackingType::Int,
        })
    );
    Ok(())
}

#[test]
fn rejects_implicit_integer_to_enum_conversion() {
    let script = parse_text(
        SourceId::new(141),
        "enum LogLevel { Trace } void take(LogLevel level); void main() { take(0); }",
        Some(&test_langspec()),
    )
    .expect("enum script should parse");
    let error = analyze_script(&script, Some(&test_langspec()))
        .expect_err("raw integer should not satisfy a strong enum parameter");
    assert_eq!(error.code, crate::CompilerErrorCode::InvalidEnumOperation);
}

#[test]
fn validates_safe_backing_to_enum_conversions() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(146),
        r#"
            enum State { Off = 1, On = 2 }
            enum Phase : string { Before = "before", After = "after" }
            void main() {
                int raw_state = 2;
                string raw_phase = "after";
                State known = State(1);
                State state = State(raw_state, State::Off);
                Phase phase = Phase(raw_phase, Phase::Before);
            }
        "#,
        Some(&test_langspec()),
    )?;
    analyze_script(&script, Some(&test_langspec()))?;
    Ok(())
}

#[test]
fn rejects_unsafe_backing_to_enum_conversions() {
    let cases = [
        "enum State { Off, On } void main() { int raw = 1; State value = State(raw); }",
        "enum State { Off, On } void main() { State value = State(42); }",
        "enum State { Off, On } enum Other { None } void main() { int raw = 1; State value = \
         State(raw, Other::None); }",
        "enum Phase : string { Before = \"before\" } void main() { string raw = \"after\"; Phase \
         value = Phase(raw); }",
    ];

    for (index, source) in cases.into_iter().enumerate() {
        let script = parse_text(
            SourceId::new(147 + u32::try_from(index).expect("test index fits u32")),
            source,
            Some(&test_langspec()),
        )
        .expect("enum conversion case should parse");
        let error = analyze_script(&script, Some(&test_langspec()))
            .expect_err("unsafe enum conversion should fail");
        assert_eq!(error.code, crate::CompilerErrorCode::InvalidEnumOperation);
    }
}

#[test]
fn enum_discriminants_can_use_plain_const_globals() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(145),
        "const int BASE = 4; const string PREFIX = \"event.\"; enum Number { Value = BASE + 1 } \
         enum Name : string { Value = PREFIX + \"before\" } void main() {}",
        Some(&test_langspec()),
    )?;
    let model = analyze_script(&script, Some(&test_langspec()))?;
    assert_eq!(
        model
            .enums
            .get("Number")
            .and_then(|declaration| declaration.variants.first())
            .map(|variant| &variant.value),
        Some(&crate::SemanticEnumValue::Int(5))
    );
    assert_eq!(
        model
            .enums
            .get("Name")
            .and_then(|declaration| declaration.variants.first())
            .map(|variant| &variant.value),
        Some(&crate::SemanticEnumValue::String(crate::ScriptString::new(
            b"event.before".to_vec()
        )))
    );
    Ok(())
}

#[test]
fn validates_exhaustive_enum_matches_with_alternatives_guards_and_blocks()
-> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(142),
        r#"
            enum LogLevel { Trace, Debug, Info }
            int should_group();
            int describe(LogLevel level) {
                return match level {
                    LogLevel::Trace => 1,
                    LogLevel::Debug | LogLevel::Info if should_group() => {
                        int result = 2;
                        result
                    },
                    _ => 3,
                };
            }
            void main() { int result = describe(LogLevel::Info); }
        "#,
        Some(&test_langspec()),
    )?;
    let model = analyze_script(&script, Some(&test_langspec()))?;
    assert_eq!(
        model
            .functions
            .get("describe")
            .map(|function| &function.return_type),
        Some(&SemanticType::Int)
    );
    Ok(())
}

#[test]
fn rejects_non_exhaustive_enum_matches() {
    let script = parse_text(
        SourceId::new(143),
        "enum State { Off, On } int value(State state) { return match state { State::Off => 0, }; \
         } void main() {}",
        Some(&test_langspec()),
    )
    .expect("match script should parse");
    let error = analyze_script(&script, Some(&test_langspec()))
        .expect_err("missing enum variant should fail exhaustiveness checking");
    assert_eq!(error.code, crate::CompilerErrorCode::NonExhaustiveMatch);
    assert!(error.message.contains("State::On"));
}

#[test]
fn rejects_invalid_extended_enum_programs_with_specific_diagnostics() {
    let cases = [
        (
            "enum Word : string { Missing } void main() {}",
            crate::CompilerErrorCode::InvalidEnumDiscriminant,
        ),
        (
            "enum State { Off = 0, On = 0 } void main() {}",
            crate::CompilerErrorCode::DuplicateEnumDefinition,
        ),
        (
            "enum State { #[default] Off, #[default] On } void main() {}",
            crate::CompilerErrorCode::MultipleEnumDefaults,
        ),
        (
            "type Left = Right; type Right = Left; void main() {}",
            crate::CompilerErrorCode::InvalidTypeAlias,
        ),
        (
            "static_assert(0, \"contract broke\"); void main() {}",
            crate::CompilerErrorCode::StaticAssertionFailed,
        ),
        (
            "enum State { Off, On } void main() { State state; int value = state + 1; }",
            crate::CompilerErrorCode::InvalidEnumOperation,
        ),
        (
            "enum State { Off, On } int f(State state) { return match state { State::Off => 0, \
             State::Off => 1, State::On => 2, }; } void main() {}",
            crate::CompilerErrorCode::UnreachableMatchArm,
        ),
        (
            "enum State { Off, On } int f(State state) { return match state { State::Off => 0, \
             State::On => \"wrong\", }; } void main() {}",
            crate::CompilerErrorCode::InvalidMatch,
        ),
        (
            "enum State { Off, On } int f(State state) { return match state { State::Off if 0 => \
             0, _ => 1, }; } void main() {}",
            crate::CompilerErrorCode::UnreachableMatchArm,
        ),
        (
            "enum State { Off, On } int f(State state) { return match state { _ => 0, State::On \
             => 1, }; } void main() {}",
            crate::CompilerErrorCode::UnreachableMatchArm,
        ),
    ];

    for (index, (source, expected)) in cases.into_iter().enumerate() {
        let script = parse_text(
            SourceId::new(200 + u32::try_from(index).expect("test index fits u32")),
            source,
            Some(&test_langspec()),
        )
        .unwrap_or_else(|error| panic!("case {index} should parse: {error}"));
        let error = match analyze_script(&script, Some(&test_langspec())) {
            Ok(_) => panic!("case {index} should fail semantic analysis"),
            Err(error) => error,
        };
        assert_eq!(error.code, expected, "case {index}: {}", error.message);
    }
}

#[test]
fn static_assert_reports_the_user_message() {
    let script = parse_text(
        SourceId::new(220),
        "static_assert(0, \"contract broke\"); void main() {}",
        Some(&test_langspec()),
    )
    .expect("static assertion should parse");
    let error =
        analyze_script(&script, Some(&test_langspec())).expect_err("false assertion should fail");
    assert_eq!(error.message, "contract broke");
}

#[test]
fn rejects_optional_parameter_ordering() {
    let script = parse_text(
        SourceId::new(41),
        "void helper(int n = 1, int m); void main() { return; }",
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let error = analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::NonOptionalParameterCannotFollowOptionalParameter
    );
}

#[test]
fn folds_constant_globals_and_uses_default_constant_values()
-> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(46),
        r#"
            const int MASK = (1 + 2) * 4;
            const int ZERO;
            const string LABEL = "ab" + "cd";
            const int PICK = TRUE ? 7 : 9;
            void main() {
                int x = MASK;
                int y = ZERO;
                string s = LABEL;
                int z = PICK;
            }
        "#,
        Some(&test_langspec()),
    )?;

    let model = analyze_script(&script, Some(&test_langspec()))?;
    assert_eq!(
        model.globals.get("MASK").map(|global| global.is_const),
        Some(true)
    );
    assert_eq!(
        model.globals.get("ZERO").map(|global| global.is_const),
        Some(true)
    );
    assert_eq!(
        model.globals.get("LABEL").map(|global| global.is_const),
        Some(true)
    );
    assert_eq!(
        model.globals.get("PICK").map(|global| global.is_const),
        Some(true)
    );
    Ok(())
}

#[test]
fn accepts_case_labels_backed_by_const_globals() {
    let script = parse_text(
        SourceId::new(47),
        r#"
            const int CASE_A = 1 + 2;
            const int CASE_B = FALSE ? 8 : 4;
            void main() {
                int n = 0;
                switch (n) {
                    case CASE_A:
                        break;
                    case CASE_B:
                        break;
                    default:
                        break;
                }
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");

    analyze_script(&script, Some(&test_langspec()))
        .expect("const global should be valid in case label");
}

#[test]
fn accepts_function_defaults_backed_by_const_globals_and_expressions() {
    let script = parse_text(
        SourceId::new(48),
        r#"
            const int EXECUTE_END = 1;
            const int POLICY_DEFAULT = EXECUTE_END + 2;
            void helper(int nValue = EXECUTE_END, int nPolicy = POLICY_DEFAULT) {
                return;
            }
            void main() {
                helper();
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");

    analyze_script(&script, Some(&test_langspec()))
        .expect("const global defaults should resolve in function signatures");
}

#[test]
fn accepts_string_case_labels_by_upstream_hash_rule() {
    let script = parse_text(
        SourceId::new(49),
        r#"
            const string LABEL = "abc";
            void main() {
                int n = 0;
                switch (n) {
                    case "abc":
                        break;
                    case LABEL:
                        break;
                }
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let error = analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::MultipleCaseConstantStatementsWithinSwitch
    );
}

#[test]
fn rejects_duplicate_default_labels_within_one_switch() {
    let script = parse_text(
        SourceId::new(50),
        r#"
            void main() {
                int n = 0;
                switch (n) {
                    default:
                        break;
                    default:
                        break;
                }
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let error = analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::MultipleDefaultStatementsWithinSwitch
    );
}

#[test]
fn rejects_case_and_default_labels_that_jump_over_live_declarations() {
    let case_script = parse_text(
        SourceId::new(51),
        r#"
            void main() {
                int n = 0;
                switch (n) {
                    int x = 1;
                    case 1:
                        break;
                }
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let case_error =
        analyze_script(&case_script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        case_error.code,
        crate::CompilerErrorCode::JumpingOverDeclarationStatementsCaseDisallowed
    );

    let default_script = parse_text(
        SourceId::new(52),
        r#"
            void main() {
                int n = 0;
                switch (n) {
                    int x = 1;
                    default:
                        break;
                }
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let default_error =
        analyze_script(&default_script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        default_error.code,
        crate::CompilerErrorCode::JumpingOverDeclarationStatementsDefaultDisallowed
    );
}

#[test]
fn duplicate_case_detection_is_scoped_to_the_innermost_switch() {
    let script = parse_text(
        SourceId::new(53),
        r#"
            void main() {
                int n = 0;
                switch (n) {
                    case 1:
                        switch (n) {
                            case 1:
                                break;
                            default:
                                break;
                        }
                        break;
                    default:
                        break;
                }
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");

    analyze_script(&script, Some(&test_langspec()))
        .expect("nested switch labels should be tracked independently");
}

#[test]
fn parser_accepts_constant_function_default_expressions_like_upstream() {
    let script = parse_text(
        SourceId::new(48),
        "void helper(int n = 1 + 2); void main() {}",
        Some(&test_langspec()),
    )
    .expect("parser should accept constant default expressions");

    analyze_script(&script, Some(&test_langspec()))
        .expect("semantic analysis should accept constant default expressions");
}

#[test]
fn rejects_undefined_identifiers_and_bad_field_access() {
    let script = parse_text(
        SourceId::new(42),
        "struct Foo { int value; }; void main() { struct Foo f; int x = f.missing; }",
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let error = analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
    assert!(matches!(
        error.code,
        crate::CompilerErrorCode::UndefinedFieldInStructure
            | crate::CompilerErrorCode::UndefinedIdentifier
    ));
}

#[test]
fn action_parameters_require_direct_void_calls() {
    let valid = parse_text(
        SourceId::new(54),
        "void helper() {} void main() { DelayCommand(1.0, helper()); }",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    analyze_script(&valid, Some(&test_langspec()))
        .expect("void call should be valid for action parameter");

    let invalid = parse_text(
        SourceId::new(55),
        "void main() { DelayCommand(1.0, EffectDamage(1)); }",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let error = analyze_script(&invalid, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::DeclarationDoesNotMatchParameters
    );
}

#[test]
fn function_name_reuse_requires_identical_parameters_but_not_return_types() {
    let mismatch = parse_text(
        SourceId::new(56),
        "void helper(int n); void helper(float n); void main() {}",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let mismatch_error =
        analyze_script(&mismatch, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        mismatch_error.code,
        crate::CompilerErrorCode::FunctionImplementationAndDefinitionDiffer
    );

    let return_mismatch = parse_text(
        SourceId::new(65),
        "int helper(int n); void helper(int n) {} void main() {}",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let return_mismatch_model = analyze_script(&return_mismatch, Some(&test_langspec()))
        .expect("native compiler accepts mismatched return types");
    assert_eq!(
        return_mismatch_model
            .functions
            .get("helper")
            .map(|function| &function.return_type),
        Some(&SemanticType::Int)
    );

    let duplicate_impl = parse_text(
        SourceId::new(57),
        "void helper(int n) {} void helper(int n) {} void main() {}",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let duplicate_impl_error =
        analyze_script(&duplicate_impl, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        duplicate_impl_error.code,
        crate::CompilerErrorCode::DuplicateFunctionImplementation
    );
}

#[test]
fn function_redeclarations_may_add_or_remove_trailing_defaults_like_upstream() {
    let later_default = parse_text(
        SourceId::new(60),
        "void helper(int n); void helper(int n = 1) {} void main() {}",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    analyze_script(&later_default, Some(&test_langspec()))
        .expect("later implementation default should be accepted");

    let earlier_default = parse_text(
        SourceId::new(61),
        "void helper(int n = 1); void helper(int n) {} void main() {}",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let earlier_default_semantic = analyze_script(&earlier_default, Some(&test_langspec()))
        .expect("later implementation without default should be accepted");
    let helper = earlier_default_semantic
        .functions
        .get("helper")
        .expect("helper should be present in the semantic model");
    let first_parameter = helper
        .parameters
        .first()
        .expect("helper should have one parameter");
    assert_eq!(
        first_parameter.default,
        Some(Literal::Integer(1)),
        "forward declaration defaults should survive into the merged signature"
    );

    let renamed_parameter = parse_text(
        SourceId::new(64),
        "int helper(int nDurationType); int helper(int nDurationCompare) { return \
         nDurationCompare; } void main() {}",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    analyze_script(&renamed_parameter, Some(&test_langspec()))
        .expect("implementation parameter names should be visible inside the body");
}

#[test]
fn nested_scopes_may_shadow_outer_names_but_same_scope_duplicates_fail() {
    let shadowing = parse_text(
        SourceId::new(62),
        "int g = 1; void main() { int x = g; { int x = 2; int y = x; } int z = x; }",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    analyze_script(&shadowing, Some(&test_langspec()))
        .expect("nested scopes should be allowed to shadow outer names");

    let duplicate = parse_text(
        SourceId::new(63),
        "void main() { int x = 1; int x = 2; }",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let duplicate_error =
        analyze_script(&duplicate, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        duplicate_error.code,
        crate::CompilerErrorCode::VariableAlreadyUsedWithinScope
    );
}

#[test]
fn local_is_visible_inside_its_own_initializer_like_upstream() {
    let script = parse_text(
        SourceId::new(66),
        "void main() { int value = value = 1; }",
        Some(&test_langspec()),
    )
    .expect("script should parse");

    analyze_script(&script, Some(&test_langspec()))
        .expect("self-referential initializer should analyze");
}

#[test]
fn function_body_scope_may_shadow_parameter_names() {
    let script = parse_text(
        SourceId::new(65),
        "int helper(object oSpellTarget) { object oSpellTarget = OBJECT_SELF; return TRUE; } void \
         main() {}",
        Some(&test_langspec()),
    )
    .expect("script should parse");
    analyze_script(&script, Some(&test_langspec()))
        .expect("function body locals should be allowed to shadow parameter names");
}

#[test]
fn rejects_local_const_declarations_like_upstream() {
    let script = parse_text(
        SourceId::new(43),
        "void main() { const int x = 1; }",
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let error = analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::ConstKeywordCannotBeUsedOnNonGlobalVariables
    );
}

#[test]
fn rejects_missing_return_paths() {
    let script = parse_text(
        SourceId::new(44),
        "int main() { if (TRUE) { return 1; } }",
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let error = analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::NotAllControlPathsReturnAValue
    );
}

#[test]
fn validates_required_entrypoints_when_requested() {
    let script = parse_text(
        SourceId::new(45),
        "int helper() { return 1; }",
        Some(&test_langspec()),
    )
    .expect("script should parse");

    let error = analyze_script_with_options(
        &script,
        Some(&test_langspec()),
        SemanticOptions {
            require_entrypoint:       true,
            allow_conditional_script: false,
        },
    )
    .expect_err("analysis should fail");
    assert_eq!(error.code, crate::CompilerErrorCode::NoFunctionMainInScript);
}

#[test]
fn missing_conditional_and_main_reports_legacy_main_error() {
    let script = parse_text(SourceId::new(46), "", Some(&test_langspec()))
        .expect("empty script should parse");

    let error = analyze_script_with_options(
        &script,
        Some(&test_langspec()),
        SemanticOptions {
            require_entrypoint:       true,
            allow_conditional_script: true,
        },
    )
    .expect_err("analysis should fail");
    assert_eq!(error.code, crate::CompilerErrorCode::NoFunctionMainInScript);
}

#[test]
fn enforces_the_legacy_32_parameter_limit() {
    let parameters = (0..33)
        .map(|index| format!("int value{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!("void oversized({parameters}) {{}} void main() {{}}");
    let script = parse_text(SourceId::new(57), &source, Some(&test_langspec()))
        .expect("33 parameters should be syntactically valid");

    let error = analyze_script(&script, Some(&test_langspec())).expect_err("analysis should fail");
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::TooManyParametersOnFunction
    );
}

#[test]
fn rejects_break_and_continue_outside_control_flow() {
    for (source_id, source) in [
        (58, "void main() { break; }"),
        (59, "void main() { continue; }"),
    ] {
        let script = parse_text(SourceId::new(source_id), source, Some(&test_langspec()))
            .expect("statement should parse");
        let error = analyze_script(&script, Some(&test_langspec()))
            .expect_err("out-of-context control flow should fail");
        assert_eq!(
            error.code,
            crate::CompilerErrorCode::BreakOutsideOfLoopOrCaseStatement
        );
    }
}
