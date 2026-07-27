use super::{OptimizationFlags, meld_instructions, optimize_hir};
use crate::{
    BuiltinConstant, BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue, LangSpec,
    NcsAuxCode, NcsInstruction, NcsOpcode, SourceId, compile_hir_to_ncs, decode_ncs_instructions,
    lower_to_hir, parse_text,
};

fn test_langspec() -> LangSpec {
    LangSpec {
        engine_num_structures: 0,
        engine_structures:     vec![],
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
        ],
        functions:             vec![BuiltinFunction {
            name:        "GetValue".to_string(),
            return_type: BuiltinType::Int,
            parameters:  vec![BuiltinParameter {
                name:    "nValue".to_string(),
                ty:      BuiltinType::Int,
                default: None,
            }],
        }],
    }
}

#[test]
fn o1_eliminates_dead_functions_in_loader_discovery_order() {
    let script = parse_text(
        SourceId::new(1),
        r#"
            int NeverUsed() { return 9; }
            int Leaf() { return 7; }
            int Mid() { return Leaf(); }
            int StartingConditional() { return Mid(); }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let semantic =
        crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
    let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
        .expect("HIR lowering should succeed");

    let optimized = optimize_hir(&hir, Some(&test_langspec()), OptimizationFlags::O1);
    let names = optimized
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["StartingConditional", "Mid", "Leaf"]);
}

#[test]
fn dead_function_pass_prefers_implementation_over_later_declaration() {
    let script = parse_text(
        SourceId::new(4),
        r#"
            int Helper() { return 7; }
            int Helper();
            int StartingConditional() { return Helper(); }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let semantic =
        crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
    let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
        .expect("HIR lowering should succeed");

    let optimized = optimize_hir(&hir, Some(&test_langspec()), OptimizationFlags::O1);
    let helper = optimized
        .functions
        .iter()
        .find(|function| function.name == "Helper")
        .expect("reachable helper should survive");

    assert!(helper.body.is_some());
}

#[test]
fn dead_function_discovery_follows_right_to_left_argument_emission() {
    let script = parse_text(
        SourceId::new(5),
        r#"
            int Left() { return 1; }
            int Right() { return 2; }
            int Combine(int left, int right) { return left + right; }
            int StartingConditional() { return Combine(Left(), Right()); }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let semantic =
        crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
    let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
        .expect("HIR lowering should succeed");

    let optimized = optimize_hir(&hir, Some(&test_langspec()), OptimizationFlags::O1);
    let names = optimized
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec!["StartingConditional", "Right", "Left", "Combine"]
    );
}

#[test]
fn o2_dead_branch_trimming_removes_calls_from_constant_false_if_branches() {
    let script = parse_text(
        SourceId::new(2),
        r#"
            int Dead() { return 0; }
            int Live() { return 1; }
            int StartingConditional() {
                if (FALSE) {
                    return Dead();
                } else {
                    return Live();
                }
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let semantic =
        crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
    let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
        .expect("HIR lowering should succeed");

    let optimized = optimize_hir(&hir, Some(&test_langspec()), OptimizationFlags::O2);
    let names = optimized
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["StartingConditional", "Live"]);
}

#[test]
fn o3_melds_local_constant_initializer_pattern() {
    let script = parse_text(
        SourceId::new(3),
        r#"
            void main() {
                int nValue = 3;
            }
        "#,
        Some(&test_langspec()),
    )
    .expect("script should parse");
    let semantic =
        crate::analyze_script(&script, Some(&test_langspec())).expect("script should analyze");
    let hir = lower_to_hir(&script, &semantic, Some(&test_langspec()))
        .expect("HIR lowering should succeed");

    let o0 = decode_ncs_instructions(
        &compile_hir_to_ncs(&hir, Some(&test_langspec()), OptimizationFlags::O0)
            .expect("O0 compile should succeed"),
    )
    .expect("O0 output should decode");
    let o3 = decode_ncs_instructions(
        &compile_hir_to_ncs(&hir, Some(&test_langspec()), OptimizationFlags::O3)
            .expect("O3 compile should succeed"),
    )
    .expect("O3 output should decode");

    assert!(
        o0.iter()
            .any(|instruction| instruction.opcode == NcsOpcode::RunstackAdd),
        "O0 should preserve the local initializer stack dance",
    );
    assert!(
        !o3.iter().any(|instruction| {
            instruction.opcode == NcsOpcode::Assignment
                && instruction.auxcode == NcsAuxCode::TypeVoid
        }),
        "O3 should meld the local constant initializer pattern",
    );
}

#[test]
fn meld_instructions_only_rewrites_the_upstream_active_pattern() {
    let instructions = vec![
        NcsInstruction {
            opcode:  NcsOpcode::RunstackAdd,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   Vec::new(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   3_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Assignment,
            auxcode: NcsAuxCode::TypeVoid,
            extra:   {
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&(-8_i32).to_be_bytes());
                bytes.extend_from_slice(&(4_u16).to_be_bytes());
                bytes
            },
        },
        NcsInstruction {
            opcode:  NcsOpcode::ModifyStackPointer,
            auxcode: NcsAuxCode::None,
            extra:   (-4_i32).to_be_bytes().to_vec(),
        },
    ];

    let optimized = meld_instructions(instructions);
    assert_eq!(optimized.len(), 1);
    assert_eq!(
        optimized.first().map(|instruction| instruction.opcode),
        Some(NcsOpcode::Constant)
    );
}
