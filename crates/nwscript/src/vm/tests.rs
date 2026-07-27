use std::{cell::RefCell, collections::HashMap, io::Cursor, rc::Rc};

use super::{Vm, VmEngineStructureValue, VmRunOptions, VmScript, VmStepOutcome, VmValue};
use crate::{
    CompileOptions, InMemoryScriptResolver, NcsAuxCode, NcsInstruction, NcsOpcode, SourceId,
    SourceMap, compile_script, compile_script_with_source_map, load_source_bundle, parse_langspec,
    parse_text, read_ndb,
};

fn compile_debug_script(
    file_name: &str,
    source: &[u8],
) -> Result<(Vec<u8>, crate::Ndb), Box<dyn std::error::Error>> {
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file(file_name.to_string(), source.to_vec());
    let script = parse_text(root_id, std::str::from_utf8(source)?, None)?;
    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        None,
        CompileOptions::default(),
    )?;
    let ndb_bytes = artifacts
        .ndb
        .ok_or("compile_script_with_source_map did not emit NDB bytes")?;
    let mut reader = Cursor::new(ndb_bytes);
    let ndb = read_ndb(&mut reader)?;
    Ok((artifacts.ncs, ndb))
}

#[test]
fn runs_manual_integer_program() -> Result<(), Box<dyn std::error::Error>> {
    let instructions = vec![
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   7_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   35_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Add,
            auxcode: NcsAuxCode::TypeTypeIntegerInteger,
            extra:   Vec::new(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        },
    ];
    let mut script = VmScript::from_instructions(instructions, "arith");

    script.run(&Vm::new())?;

    assert_eq!(script.stack(), &[VmValue::Int(42)]);
    Ok(())
}

#[test]
fn invokes_registered_action_command() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { PrintInteger(42); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let called = Rc::new(RefCell::new(None));
    let mut vm = Vm::new();
    {
        let called = Rc::clone(&called);
        vm.define_command(0, move |script, _command, argc| {
            assert_eq!(argc, 1);
            *called.borrow_mut() = Some(script.pop_int()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "compiled-main")?;
    runtime.run(&vm)?;

    assert_eq!(*called.borrow(), Some(42));
    Ok(())
}

#[test]
fn preserves_non_utf8_script_strings_through_compile_and_vm()
-> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void Capture(string sValue);")?;
    let script = parse_text(
        SourceId::new(101),
        r#"void main() { Capture("\xFF\x80"); }"#,
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let captured = Rc::new(RefCell::new(None));
    let mut vm = Vm::new();
    {
        let captured = Rc::clone(&captured);
        vm.define_simple_command(0, move |script| {
            *captured.borrow_mut() = Some(script.pop_string()?.into_bytes());
            Ok(())
        });
    }
    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "raw-string")?;
    runtime.run(&vm)?;

    assert_eq!(*captured.borrow(), Some(vec![0xff, 0x80]));
    Ok(())
}

#[test]
fn loads_and_runs_script_through_source_bundle_pipeline() -> Result<(), Box<dyn std::error::Error>>
{
    let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
    let mut resolver = InMemoryScriptResolver::new();
    resolver.insert_source("main", "void main() { PrintInteger(7); }");
    let bundle = load_source_bundle(&resolver, "main", crate::SourceLoadOptions::default())?;
    let artifacts =
        crate::compile_source_bundle(&bundle, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let seen = Rc::clone(&seen);
        vm.define_simple_command(0, move |script| {
            seen.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "bundle-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &[7]);
    Ok(())
}

#[test]
fn executes_compiled_vector_arithmetic() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void PrintVector(vector v);")?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { PrintVector(([1.0, 2.0, 3.0] + [4.0, 5.0, 6.0]) * 2.0); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let seen = Rc::clone(&seen);
        vm.define_simple_command(0, move |script| {
            seen.borrow_mut().push(script.pop_vector()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "vector-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &[[10.0, 14.0, 18.0]]);
    Ok(())
}

#[test]
fn executes_compiled_mixed_numeric_arithmetic() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void PrintFloat(float f);")?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { PrintFloat(1 + 2.5); PrintFloat(5.5 - 2); PrintFloat(3 * 1.5); \
         PrintFloat(9.0 / 2); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let seen = Rc::clone(&seen);
        vm.define_simple_command(0, move |script| {
            seen.borrow_mut().push(script.pop_float()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "mixed-numeric-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &[3.5, 3.5, 4.5, 4.5]);
    Ok(())
}

#[test]
fn executes_compiled_mixed_numeric_comparisons() -> Result<(), Box<dyn std::error::Error>> {
    let instructions = vec![
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   1_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeFloat,
            extra:   2.5_f32.to_bits().to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Lt,
            auxcode: NcsAuxCode::TypeTypeIntegerFloat,
            extra:   Vec::new(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeFloat,
            extra:   2.5_f32.to_bits().to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   1_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Gt,
            auxcode: NcsAuxCode::TypeTypeFloatInteger,
            extra:   Vec::new(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   3_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeFloat,
            extra:   3.0_f32.to_bits().to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Equal,
            auxcode: NcsAuxCode::TypeTypeIntegerFloat,
            extra:   Vec::new(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeFloat,
            extra:   4.5_f32.to_bits().to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   4_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::NotEqual,
            auxcode: NcsAuxCode::TypeTypeFloatInteger,
            extra:   Vec::new(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        },
    ];
    let mut runtime = VmScript::from_instructions(instructions, "mixed-compare-manual");
    runtime.run(&Vm::new())?;

    assert_eq!(
        runtime.stack(),
        &[
            VmValue::Int(1),
            VmValue::Int(1),
            VmValue::Int(1),
            VmValue::Int(1),
        ]
    );
    Ok(())
}

#[test]
fn grows_stack_with_positive_movsp() -> Result<(), Box<dyn std::error::Error>> {
    let instructions = vec![
        NcsInstruction {
            opcode:  NcsOpcode::ModifyStackPointer,
            auxcode: NcsAuxCode::None,
            extra:   4_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        },
    ];
    let mut script = VmScript::from_instructions(instructions, "movsp-grow");

    script.run(&Vm::new())?;

    assert_eq!(script.stack(), &[VmValue::Int(0)]);
    Ok(())
}

#[test]
fn steps_manual_program_instruction_by_instruction() -> Result<(), Box<dyn std::error::Error>> {
    let instructions = vec![
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   7_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   35_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Add,
            auxcode: NcsAuxCode::TypeTypeIntegerInteger,
            extra:   Vec::new(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        },
    ];
    let mut script = VmScript::from_instructions(instructions, "step-arith");
    let vm = Vm::new();

    assert_eq!(
        script
            .current_instruction()
            .map(|instruction| instruction.opcode),
        Some(NcsOpcode::Constant)
    );
    assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
    assert_eq!(script.stack(), &[VmValue::Int(7)]);

    assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
    assert_eq!(script.stack(), &[VmValue::Int(7), VmValue::Int(35)]);

    assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
    assert_eq!(script.stack(), &[VmValue::Int(42)]);

    assert_eq!(vm.step(&mut script)?, VmStepOutcome::Halted);
    assert_eq!(script.stack(), &[VmValue::Int(42)]);
    Ok(())
}

#[test]
fn steps_over_and_out_of_compiled_user_calls() -> Result<(), Box<dyn std::error::Error>> {
    let source = br#"int AddOne(int x) {
return x + 1;
}

int Twice(int x) {
int first = AddOne(x);
int second = AddOne(x);
return first + second;
}
"#;
    let (ncs, ndb) = compile_debug_script("debug_calls.nss", source)?;
    let vm = Vm::new();
    let mut script = VmScript::from_bytes_with_ndb(&ncs, "debug-calls", &ndb)?;
    script.prepare_function_call(&ndb, "Twice", &[VmValue::Int(2)])?;

    for _ in 0..32 {
        if script
            .current_instruction()
            .map(|instruction| instruction.opcode)
            == Some(NcsOpcode::Jsr)
        {
            break;
        }
        assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
    }

    let before = script.ip();
    assert_eq!(
        script.current_function().map(|function| function.name),
        Some("Twice".to_string())
    );
    assert_eq!(
        vm.step_over(&mut script, VmRunOptions::default())?,
        VmStepOutcome::Running
    );
    assert!(script.ip() > before);
    assert_eq!(
        script.current_function().map(|function| function.name),
        Some("Twice".to_string())
    );

    assert_eq!(
        vm.run_until_function(&mut script, "AddOne", VmRunOptions::default())?,
        VmStepOutcome::Running
    );
    assert_eq!(
        script.current_function().map(|function| function.name),
        Some("AddOne".to_string())
    );

    assert_eq!(
        vm.step_out(&mut script, VmRunOptions::default())?,
        VmStepOutcome::Running
    );
    assert_eq!(
        script.current_function().map(|function| function.name),
        Some("Twice".to_string())
    );
    Ok(())
}

#[test]
fn exposes_source_locations_and_runs_until_lines() -> Result<(), Box<dyn std::error::Error>> {
    let source = br#"int AddOne(int x) {
return x + 1;
}

int Twice(int x) {
int first = AddOne(x);
int second = AddOne(x);
return first + second;
}
"#;
    let (ncs, ndb) = compile_debug_script("debug_lines.nss", source)?;
    let vm = Vm::new();
    let mut script = VmScript::from_bytes_with_ndb(&ncs, "debug-lines", &ndb)?;
    script.prepare_function_call(&ndb, "Twice", &[VmValue::Int(2)])?;
    let root_file_index = ndb
        .files
        .iter()
        .position(|file| file.name == "debug_lines.nss")
        .ok_or("missing root file entry")?;
    let mut root_lines = ndb
        .lines
        .iter()
        .filter(|line| line.file_num == root_file_index)
        .map(|line| line.line_num)
        .collect::<Vec<_>>();
    root_lines.sort_unstable();
    root_lines.dedup();
    let first_line = *root_lines.first().ok_or("missing root source lines")?;
    let second_line = *root_lines
        .iter()
        .find(|line| **line > first_line)
        .ok_or("missing second root source line")?;

    assert_eq!(
        vm.run_until_line(
            &mut script,
            "debug_lines.nss",
            first_line,
            VmRunOptions::default()
        )?,
        VmStepOutcome::Running
    );
    let location = script
        .current_source_location()
        .ok_or("missing first source location")?;
    assert_eq!(location.file_name, "debug_lines.nss");
    assert!(location.is_root);
    assert_eq!(location.line_number, first_line);

    assert_eq!(
        vm.run_until_line(
            &mut script,
            "debug_lines.nss",
            second_line,
            VmRunOptions::default()
        )?,
        VmStepOutcome::Running
    );
    let location = script
        .current_source_location()
        .ok_or("missing second source location")?;
    assert_eq!(location.line_number, second_line);
    assert_eq!(location.file_name, "debug_lines.nss");
    Ok(())
}

#[test]
fn executes_compiled_struct_equality() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
    let source = r#"
        struct Pair {
            int a;
            int b;
        };

        void main() {
            struct Pair left;
            struct Pair right;
            left.a = 1;
            left.b = 2;
            right.a = 1;
            right.b = 2;
            PrintInteger(left == right);
        }
    "#;
    let script = parse_text(SourceId::new(0), source, Some(&langspec))?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let seen = Rc::clone(&seen);
        vm.define_simple_command(0, move |script| {
            seen.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "struct-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &[1]);
    Ok(())
}

#[test]
fn executes_nested_int_and_string_enum_matches_with_block_arms()
-> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec(
        "nwscript",
        "void PrintInteger(int n);\nvoid PrintString(string s);",
    )?;
    let source = r#"
        enum Mode { Off, On, #[default] Unknown = 5 }
        enum Word : string { Hello = "hello", Goodbye = "goodbye" }
        struct Holder { Mode mode; };
        int should_use_first() { return 0; }

        int choose(Mode mode) {
            return 10 + match mode {
                Mode::Off => 1,
                Mode::On if should_use_first() => 2,
                Mode::On => { int adjustment = 3; adjustment + 1 },
                Mode::Unknown => 5,
            };
        }

        int early(Mode mode) {
            return match mode {
                Mode::Off => 0,
                Mode::On => { return 9; }
                Mode::Unknown => 5,
            };
        }

        int string_switch(Word word) {
            switch (word) {
                case Word::Hello: return 1;
                case Word::Goodbye: return 2;
            }
            return 0;
        }

        int statement_return(Mode mode) {
            match mode {
                Mode::Off => { return 3; }
                Mode::On => { return 4; }
                Mode::Unknown => { return 5; }
            }
        }

        int break_from_match() {
            int result = 0;
            while (1) {
                match Mode::On {
                    Mode::Off => {}
                    Mode::On => { result = 8; break; }
                    Mode::Unknown => {}
                }
                result = 1;
            }
            return result;
        }

        void main() {
            struct Holder holder;
            PrintInteger(choose(Mode::On));
            PrintInteger(int(holder.mode));
            PrintInteger(early(Mode::On));
            PrintInteger(string_switch(Word::Goodbye));
            PrintInteger(statement_return(Mode::On));
            PrintInteger(break_from_match());
            PrintString(match Word::Goodbye {
                Word::Hello => "wrong",
                Word::Goodbye => "right",
            });
        }
    "#;
    let script = parse_text(SourceId::new(0), source, Some(&langspec))?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let integers = Rc::new(RefCell::new(Vec::new()));
    let strings = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let integers = Rc::clone(&integers);
        vm.define_simple_command(0, move |script| {
            integers.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }
    {
        let strings = Rc::clone(&strings);
        vm.define_simple_command(1, move |script| {
            strings.borrow_mut().push(script.pop_string()?.into_bytes());
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "enum-match-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*integers.borrow(), &[14, 5, 9, 2, 4, 8]);
    assert_eq!(&*strings.borrow(), &[b"right".to_vec()]);
    Ok(())
}

#[test]
fn checked_enum_conversions_accept_declared_values_and_apply_fallbacks()
-> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec(
        "nwscript",
        "void PrintInteger(int n);\nvoid PrintString(string s);",
    )?;
    let source = r#"
        enum State { Off = 1, On = 2 }
        enum Phase : string { Before = "before", After = "after" }

        void main() {
            int valid = 2;
            int invalid = 99;
            string valid_text = "after";
            string invalid_text = "unknown";
            PrintInteger(int(State(valid, State::Off)));
            PrintInteger(int(State(invalid, State::Off)));
            PrintString(string(Phase(valid_text, Phase::Before)));
            PrintString(string(Phase(invalid_text, Phase::Before)));
        }
    "#;
    let script = parse_text(SourceId::new(0), source, Some(&langspec))?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let integers = Rc::new(RefCell::new(Vec::new()));
    let strings = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let integers = Rc::clone(&integers);
        vm.define_simple_command(0, move |script| {
            integers.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }
    {
        let strings = Rc::clone(&strings);
        vm.define_simple_command(1, move |script| {
            strings.borrow_mut().push(script.pop_string()?.into_bytes());
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "checked-enum-conversion")?;
    runtime.run(&vm)?;

    assert_eq!(&*integers.borrow(), &[2, 1]);
    assert_eq!(&*strings.borrow(), &[b"after".to_vec(), b"before".to_vec()]);
    Ok(())
}

#[test]
fn executes_compiled_user_function_calls_without_ndb() -> Result<(), Box<dyn std::error::Error>> {
    let source = br#"
        int AddOne(int nValue) {
            return nValue + 1;
        }

        void main() {
            PrintInteger(AddOne(41));
        }
    "#;
    let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
    let script = parse_text(
        SourceId::new(0),
        std::str::from_utf8(source)?,
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;
    assert!(artifacts.ndb.is_none());

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let seen = Rc::clone(&seen);
        vm.define_simple_command(0, move |script| {
            seen.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }

    vm.run_bytes(&artifacts.ncs, "user-call-main")?;

    assert_eq!(&*seen.borrow(), &[42]);
    Ok(())
}

#[test]
fn compiled_builtin_arguments_follow_native_stack_order() -> Result<(), Box<dyn std::error::Error>>
{
    let langspec = parse_langspec("nwscript", "void Assert(int condition, string message);")?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { Assert(7, \"seven\"); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let mut vm = Vm::new();
    vm.define_command(0, |script, _command, argc| {
        assert_eq!(argc, 2);
        assert_eq!(script.pop_int()?, 7);
        assert_eq!(script.pop_string()?.as_bytes(), b"seven");
        Ok(())
    });

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "builtin-argument-order")?;
    runtime.run(&vm)?;
    Ok(())
}

#[test]
fn captures_and_executes_saved_action_situations() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec(
        "nwscript",
        "void DelayCommand(float seconds, action aAction);\nvoid PrintInteger(int n);",
    )?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { int value = 42; DelayCommand(1.0, PrintInteger(value + 1)); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let delayed = Rc::new(RefCell::new(None));
    let printed = Rc::new(RefCell::new(None));
    let mut vm = Vm::new();
    {
        let delayed = Rc::clone(&delayed);
        vm.define_command(0, move |script, _command, argc| {
            assert_eq!(argc, 2);
            let seconds = script.pop_float()?;
            assert_eq!(seconds, 1.0);
            *delayed.borrow_mut() = script.saved_situation().cloned();
            Ok(())
        });
    }
    {
        let printed = Rc::clone(&printed);
        vm.define_command(1, move |script, _command, argc| {
            assert_eq!(argc, 1);
            *printed.borrow_mut() = Some(script.pop_int()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "compiled-delay")?;
    runtime.run(&vm)?;

    let saved = delayed
        .borrow()
        .clone()
        .ok_or("missing saved action situation")?;
    vm.run_situation(&saved)?;

    assert_eq!(*printed.borrow(), Some(43));
    Ok(())
}

#[test]
fn roundtrips_engine_structures_through_action_handlers() -> Result<(), Box<dyn std::error::Error>>
{
    let langspec = parse_langspec(
        "nwscript",
        "effect EffectDamage(int nAmount);\nvoid PrintEffect(effect eValue);",
    )?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { PrintEffect(EffectDamage(42)); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    vm.define_command(0, move |script, _command, argc| {
        assert_eq!(argc, 1);
        let amount = script.pop_int()?;
        script.push_engine_structure(0, VmEngineStructureValue::Word(amount as u32));
        Ok(())
    });
    {
        let seen = Rc::clone(&seen);
        vm.define_command(1, move |script, _command, argc| {
            assert_eq!(argc, 1);
            let value = script.pop_engine_structure_index(0)?;
            let Some(value) = value.as_word() else {
                return Err(super::VmError::TypeMismatch {
                    offset:   script.ip(),
                    message:  "expected word-backed effect value".to_string(),
                    expected: Some("engine structure"),
                    actual:   "engine structure",
                });
            };
            seen.borrow_mut().push(value);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "effect-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &[42]);
    Ok(())
}

#[test]
fn uses_host_defined_engine_structure_defaults_for_rsadd() -> Result<(), Box<dyn std::error::Error>>
{
    let langspec = parse_langspec(
        "nwscript",
        "#define ENGINE_NUM_STRUCTURES 8\n#define ENGINE_STRUCTURE_7 json\nvoid PrintJson(json \
         jValue);",
    )?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { json jValue; PrintJson(jValue); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    vm.define_engine_structure_default(7, VmEngineStructureValue::Text("{\"ok\":true}".into()));
    {
        let seen = Rc::clone(&seen);
        vm.define_command(0, move |script, _command, argc| {
            assert_eq!(argc, 1);
            let value = script.pop_engine_structure_index(7)?;
            let Some(value) = value.as_text() else {
                return Err(super::VmError::TypeMismatch {
                    offset:   script.ip(),
                    message:  "expected text-backed json value".to_string(),
                    expected: Some("engine structure"),
                    actual:   "engine structure",
                });
            };
            seen.borrow_mut().push(value.to_string());
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "json-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &["{\"ok\":true}".to_string()]);
    Ok(())
}

#[test]
fn uses_host_defined_engine_structure_comparison() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec(
        "nwscript",
        "effect EffectDamage(int nAmount);\nvoid PrintInteger(int n);",
    )?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { PrintInteger(EffectDamage(5) == EffectDamage(5)); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let next_handle = Rc::new(RefCell::new(100_u32));
    let effect_values = Rc::new(RefCell::new(HashMap::<u32, i32>::new()));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let next_handle = Rc::clone(&next_handle);
        let effect_values = Rc::clone(&effect_values);
        vm.define_command(0, move |script, _command, argc| {
            assert_eq!(argc, 1);
            let amount = script.pop_int()?;
            let handle = *next_handle.borrow();
            *next_handle.borrow_mut() = handle + 1;
            effect_values.borrow_mut().insert(handle, amount);
            script.push_engine_structure(0, VmEngineStructureValue::Word(handle));
            Ok(())
        });
    }
    {
        let effect_values = Rc::clone(&effect_values);
        vm.define_engine_structure_comparer(0, move |_index, lhs, rhs| {
            let Some(lhs) = lhs.as_word() else {
                return false;
            };
            let Some(rhs) = rhs.as_word() else {
                return false;
            };
            effect_values.borrow().get(&lhs) == effect_values.borrow().get(&rhs)
        });
    }
    {
        let seen = Rc::clone(&seen);
        vm.define_command(1, move |script, _command, argc| {
            assert_eq!(argc, 1);
            seen.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "effect-compare-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &[1]);
    Ok(())
}

#[test]
fn aborts_script_after_action_handler_requests_abort() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void StopNow();\nvoid PrintInteger(int n);")?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { StopNow(); PrintInteger(42); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    vm.define_command(0, move |script, _command, argc| {
        assert_eq!(argc, 0);
        script.abort();
        Ok(())
    });
    {
        let seen = Rc::clone(&seen);
        vm.define_command(1, move |script, _command, argc| {
            assert_eq!(argc, 1);
            seen.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "abort-main")?;
    runtime.run(&vm)?;

    assert!(runtime.aborted());
    assert!(seen.borrow().is_empty());
    Ok(())
}

#[test]
fn emits_instruction_trace_events_during_execution() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { PrintInteger(7); }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let traces = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new();
    {
        let traces = Rc::clone(&traces);
        vm.define_trace_hook(move |_script, event| {
            traces.borrow_mut().push((
                event.offset,
                event.instruction.opcode,
                event.instruction.auxcode,
                event.sp,
                event.bp,
            ));
        });
    }
    {
        let seen = Rc::clone(&seen);
        vm.define_command(0, move |script, _command, argc| {
            assert_eq!(argc, 1);
            seen.borrow_mut().push(script.pop_int()?);
            Ok(())
        });
    }

    let mut runtime = VmScript::from_bytes(&artifacts.ncs, "trace-main")?;
    runtime.run(&vm)?;

    assert_eq!(&*seen.borrow(), &[7]);
    let trace = traces.borrow();
    assert!(!trace.is_empty());
    assert_eq!(trace.first().map(|entry| entry.1), Some(NcsOpcode::Jsr));
    assert!(
        trace
            .iter()
            .any(|(_, opcode, auxcode, _, _)| *opcode == NcsOpcode::Constant
                && *auxcode == NcsAuxCode::TypeInteger)
    );
    assert!(
        trace
            .iter()
            .any(|(_, opcode, _, _, _)| *opcode == NcsOpcode::ExecuteCommand)
    );
    Ok(())
}

#[test]
fn rejects_runs_that_exceed_instruction_budget() -> Result<(), Box<dyn std::error::Error>> {
    let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
    let script = parse_text(
        SourceId::new(0),
        "void main() { while (1) {} }",
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    let vm = Vm::new();
    let error = vm
        .run_bytes_with_options(
            &artifacts.ncs,
            "budget-main",
            VmRunOptions {
                max_instructions: Some(16),
                ..VmRunOptions::default()
            },
        )
        .expect_err("infinite loop should exceed instruction budget");

    assert!(matches!(
        &error,
        super::VmError::InstructionLimitExceeded {
            limit: 16,
            ..
        }
    ));
    assert_eq!(
        error.code(),
        Some(crate::CompilerErrorCode::VmTooManyInstructions)
    );
    Ok(())
}

#[test]
fn maps_division_by_zero_to_native_error_code() -> Result<(), Box<dyn std::error::Error>> {
    let instructions = vec![
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   1_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   0_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Div,
            auxcode: NcsAuxCode::TypeTypeIntegerInteger,
            extra:   Vec::new(),
        },
    ];
    let mut script = VmScript::from_instructions(instructions, "divide-zero");
    let error = script
        .run(&Vm::new())
        .expect_err("division by zero should fail");
    assert!(matches!(error, super::VmError::DivideByZero { .. }));
    assert_eq!(error.code(), Some(crate::CompilerErrorCode::VmDivideByZero));
    Ok(())
}

#[test]
fn integer_edge_arithmetic_uses_defined_wrapping_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    for (opcode, rhs, expected) in [
        (NcsOpcode::Div, Some(-1_i32), i32::MIN),
        (NcsOpcode::Modulus, Some(-1_i32), 0),
        (NcsOpcode::Negation, None, i32::MIN),
    ] {
        let mut instructions = vec![NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   i32::MIN.to_be_bytes().to_vec(),
        }];
        if let Some(rhs) = rhs {
            instructions.push(NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   rhs.to_be_bytes().to_vec(),
            });
        }
        instructions.push(NcsInstruction {
            opcode,
            auxcode: if opcode == NcsOpcode::Negation {
                NcsAuxCode::TypeInteger
            } else {
                NcsAuxCode::TypeTypeIntegerInteger
            },
            extra: Vec::new(),
        });
        instructions.push(NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        });
        let mut script = VmScript::from_instructions(instructions, "integer-edge");
        script.run(&Vm::new())?;
        assert_eq!(script.stack(), &[VmValue::Int(expected)]);
    }

    let mut increment = VmScript::from_instructions(
        vec![
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   i32::MAX.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Increment,
                auxcode: NcsAuxCode::None,
                extra:   (-4_i32).to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ],
        "increment-edge",
    );
    increment.run(&Vm::new())?;
    assert_eq!(increment.stack(), &[VmValue::Int(i32::MIN)]);
    Ok(())
}

#[test]
fn movsp_checks_alignment_and_stack_budget_before_allocation() {
    let vm = Vm::new();
    let mut unaligned = VmScript::from_instructions(
        vec![NcsInstruction {
            opcode:  NcsOpcode::ModifyStackPointer,
            auxcode: NcsAuxCode::None,
            extra:   1_i32.to_be_bytes().to_vec(),
        }],
        "unaligned-movsp",
    );
    assert!(matches!(
        vm.step(&mut unaligned),
        Err(super::VmError::InvalidExtra { .. })
    ));

    let mut oversized = VmScript::from_instructions(
        vec![NcsInstruction {
            opcode:  NcsOpcode::ModifyStackPointer,
            auxcode: NcsAuxCode::None,
            extra:   8_i32.to_be_bytes().to_vec(),
        }],
        "oversized-movsp",
    );
    let error = vm
        .run_with_options(
            &mut oversized,
            VmRunOptions {
                max_stack_cells: Some(1),
                ..VmRunOptions::default()
            },
        )
        .expect_err("MOVSP must fail before exceeding the configured stack");
    assert!(matches!(
        error,
        super::VmError::StackLimitExceeded {
            cells: 2, limit: 1
        }
    ));
    assert!(oversized.stack().is_empty());

    let mut minimum = VmScript::from_instructions(
        vec![NcsInstruction {
            opcode:  NcsOpcode::ModifyStackPointer,
            auxcode: NcsAuxCode::None,
            extra:   i32::MIN.to_be_bytes().to_vec(),
        }],
        "minimum-movsp",
    );
    assert!(matches!(vm.step(&mut minimum), Ok(VmStepOutcome::Running)));

    let mut minimum_jump = VmScript::from_instructions(
        vec![NcsInstruction {
            opcode:  NcsOpcode::Jmp,
            auxcode: NcsAuxCode::None,
            extra:   i32::MIN.to_be_bytes().to_vec(),
        }],
        "minimum-jump",
    );
    assert!(matches!(
        vm.step(&mut minimum_jump),
        Err(super::VmError::InvalidInstructionPointer { .. })
    ));

    let mut minimum_stack_offset = VmScript::from_instructions(
        vec![NcsInstruction {
            opcode:  NcsOpcode::Increment,
            auxcode: NcsAuxCode::None,
            extra:   i32::MIN.to_be_bytes().to_vec(),
        }],
        "minimum-stack-offset",
    );
    assert!(matches!(
        vm.step(&mut minimum_stack_offset),
        Err(super::VmError::StackUnderflow { .. })
    ));
}

#[test]
fn maps_stack_and_recursion_budgets_to_native_error_codes() -> Result<(), Box<dyn std::error::Error>>
{
    let stack_program = vec![
        NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   1_i32.to_be_bytes().to_vec(),
        },
        NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        },
    ];
    let mut stack_script = VmScript::from_instructions(stack_program, "stack-limit");
    let stack_error = Vm::new()
        .run_with_options(
            &mut stack_script,
            VmRunOptions {
                max_stack_cells: Some(0),
                ..VmRunOptions::default()
            },
        )
        .expect_err("stack limit should fail");
    assert_eq!(
        stack_error.code(),
        Some(crate::CompilerErrorCode::VmStackOverflow)
    );

    let recursive = parse_text(
        SourceId::new(0),
        "void recurse() { recurse(); } void main() { recurse(); }",
        None,
    )?;
    let artifacts = compile_script(&recursive, None, CompileOptions::default())?;
    let recursion_error = Vm::new()
        .run_bytes_with_options(
            &artifacts.ncs,
            "recursion-limit",
            VmRunOptions {
                max_recursion_depth: Some(1),
                ..VmRunOptions::default()
            },
        )
        .expect_err("recursion limit should fail");
    assert_eq!(
        recursion_error.code(),
        Some(crate::CompilerErrorCode::VmTooManyLevelsOfRecursion)
    );
    Ok(())
}

#[test]
fn runs_named_function_calls_from_ndb_without_globals() -> Result<(), Box<dyn std::error::Error>> {
    let source = br#"
        int AddOne(int nValue) {
            return nValue + 1;
        }

        void main() {
            return;
        }
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("direct_call.nss".to_string(), source.to_vec());
    let langspec = parse_langspec("nwscript", "void Dummy();")?;
    let script = parse_text(root_id, std::str::from_utf8(source)?, Some(&langspec))?;
    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&langspec),
        CompileOptions::default(),
    )?;
    let ndb = read_ndb(&mut std::io::Cursor::new(
        artifacts.ndb.clone().ok_or("missing ndb")?,
    ))?;

    let vm = Vm::new();
    let runtime = vm.run_function_bytes(
        &artifacts.ncs,
        "direct-call",
        &ndb,
        "AddOne",
        &[VmValue::Int(41)],
    )?;

    assert_eq!(
        runtime.function_return_value(&ndb, "AddOne")?,
        Some(VmValue::Int(42))
    );
    Ok(())
}

#[test]
fn runs_struct_arguments_and_returns_from_ndb() -> Result<(), Box<dyn std::error::Error>> {
    let source = br#"
        struct Pair {
            int first;
            int second;
        };

        struct Pair Swap(struct Pair value) {
            struct Pair result;
            result.first = value.second;
            result.second = value.first;
            return result;
        }

        void main() {}
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("direct_struct.nss".to_string(), source.to_vec());
    let script = parse_text(root_id, std::str::from_utf8(source)?, None)?;
    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        None,
        CompileOptions::default(),
    )?;
    let ndb = read_ndb(&mut std::io::Cursor::new(
        artifacts.ndb.clone().ok_or("missing ndb")?,
    ))?;

    let vm = Vm::new();
    let runtime = vm.run_function_bytes(
        &artifacts.ncs,
        "direct-struct",
        &ndb,
        "Swap",
        &[VmValue::Struct(vec![VmValue::Int(10), VmValue::Int(20)])],
    )?;

    assert_eq!(
        runtime.function_return_value(&ndb, "Swap")?,
        Some(VmValue::Struct(vec![VmValue::Int(20), VmValue::Int(10)]))
    );
    Ok(())
}

#[test]
fn runs_direct_function_calls_after_bootstrapping_globals() -> Result<(), Box<dyn std::error::Error>>
{
    let source = br#"
        int GLOBAL = 1;
        int SECOND = GLOBAL + 2;

        int ReadGlobal() {
            return GLOBAL + SECOND;
        }

        void main() {
            return;
        }
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file("globals_direct_call.nss".to_string(), source.to_vec());
    let langspec = parse_langspec("nwscript", "void Dummy();")?;
    let script = parse_text(root_id, std::str::from_utf8(source)?, Some(&langspec))?;
    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&langspec),
        CompileOptions::default(),
    )?;
    let ndb = read_ndb(&mut std::io::Cursor::new(
        artifacts.ndb.clone().ok_or("missing ndb")?,
    ))?;

    let vm = Vm::new();
    let runtime = vm.run_function_bytes(
        &artifacts.ncs,
        "globals-direct-call",
        &ndb,
        "ReadGlobal",
        &[],
    )?;

    assert_eq!(
        runtime.function_return_value(&ndb, "ReadGlobal")?,
        Some(VmValue::Int(4))
    );
    Ok(())
}

#[test]
fn runs_direct_function_calls_for_globals_with_non_void_entry_loader()
-> Result<(), Box<dyn std::error::Error>> {
    let source = br#"
        int GLOBAL = 1;

        int ReadGlobal() {
            return GLOBAL;
        }

        int StartingConditional() {
            return 0;
        }
    "#;
    let mut source_map = SourceMap::new();
    let root_id = source_map.add_file(
        "globals_loader_conditional.nss".to_string(),
        source.to_vec(),
    );
    let langspec = parse_langspec("nwscript", "void Dummy();")?;
    let script = parse_text(root_id, std::str::from_utf8(source)?, Some(&langspec))?;
    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&langspec),
        CompileOptions::default(),
    )?;
    let ndb = read_ndb(&mut std::io::Cursor::new(
        artifacts.ndb.clone().ok_or("missing ndb")?,
    ))?;
    let vm = Vm::new();
    let runtime = vm.run_function_bytes(
        &artifacts.ncs,
        "globals-conditional",
        &ndb,
        "ReadGlobal",
        &[],
    )?;
    assert_eq!(
        runtime.function_return_value(&ndb, "ReadGlobal")?,
        Some(VmValue::Int(1))
    );
    Ok(())
}
