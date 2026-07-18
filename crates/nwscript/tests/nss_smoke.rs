#![allow(missing_docs)]

use std::error::Error;

use nwnrs_nwscript::prelude::*;

mod support;

use support::{load_ncs_bytes, load_nss_bytes, skip_if_game_resources_unavailable, test_error};

type TestResult = Result<(), Box<dyn Error>>;

const POSITIVE_SCRIPT: &str = "xp1_less25hp.nss";
const INCLUDE_SCRIPT: &str = "inc_mf_combat";
const NEGATIVE_SCRIPTS: &[(&str, &str)] = &[
    (
        "nw_d2_ing6.nss",
        "expected failure: template placeholder expression <<PLACE THE CONDITIONAL HERE>>",
    ),
    (
        "nw_d2_inl9.nss",
        "expected failure: template placeholder expression <<PLACE THE CONDITIONAL HERE>>",
    ),
];

#[derive(Debug, Clone)]
struct InstallScriptResolver;

impl ScriptResolver for InstallScriptResolver {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: nwnrs_types::resman::prelude::ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        if res_type != NW_SCRIPT_SOURCE_RES_TYPE {
            return Ok(None);
        }

        let candidates = if script_name.ends_with(".nss") {
            vec![script_name.to_string()]
        } else {
            vec![format!("{script_name}.nss"), script_name.to_string()]
        };

        for candidate in candidates {
            match load_nss_bytes(&candidate) {
                Ok(bytes) => return Ok(Some(bytes)),
                Err(_) => continue,
            }
        }

        Ok(None)
    }
}

fn load_installed_langspec() -> Result<LangSpec, Box<dyn Error>> {
    let langspec_source = load_nss_bytes("nwscript.nss")?;
    let langspec = parse_langspec_bytes("nwscript.nss", &langspec_source)?;
    Ok(langspec)
}

#[test]
fn installed_script_parses_and_analyzes() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let source = match load_nss_bytes(POSITIVE_SCRIPT) {
        Ok(source) => source,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let script = parse_bytes(SourceId::new(1), &source, Some(&langspec))?;
    analyze_script(&script, Some(&langspec))?;
    Ok(())
}

#[test]
fn installed_script_compiles_to_deterministic_ncs() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let source = match load_nss_bytes(POSITIVE_SCRIPT) {
        Ok(source) => source,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let script = parse_bytes(SourceId::new(2), &source, Some(&langspec))?;

    let first = compile_script(&script, Some(&langspec), CompileOptions::default())?;
    let second = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    assert_eq!(first.ncs, second.ncs);
    assert!(!decode_ncs_instructions(&first.ncs)?.is_empty());
    Ok(())
}

fn run_less_than_quarter_hit_points(
    ncs: &[u8],
    langspec: &LangSpec,
    current_hit_points: i32,
    maximum_hit_points: i32,
) -> Result<i32, Box<dyn Error>> {
    let command_id = |name: &str| {
        langspec
            .functions
            .iter()
            .position(|function| function.name == name)
            .and_then(|index| u16::try_from(index).ok())
            .ok_or_else(|| test_error(format!("missing builtin function {name}")))
    };
    let mut vm = Vm::new();
    vm.define_command(
        command_id("GetCurrentHitPoints")?,
        move |script, _, argc| {
            for _ in 0..argc {
                script.pop()?;
            }
            script.push_int(current_hit_points);
            Ok(())
        },
    );
    vm.define_command(command_id("GetMaxHitPoints")?, move |script, _, argc| {
        for _ in 0..argc {
            script.pop()?;
        }
        script.push_int(maximum_hit_points);
        Ok(())
    });
    let mut runtime = VmScript::from_bytes(ncs, "xp1_less25hp")?;
    runtime.run(&vm)?;
    match runtime.stack().last() {
        Some(VmValue::Int(value)) => Ok(*value),
        other => Err(test_error(format!(
            "conditional script did not leave an integer result: {other:?}"
        ))
        .into()),
    }
}

#[test]
fn install_backed_reference_ncs_matches_every_optimization_combination() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let source = match load_nss_bytes(POSITIVE_SCRIPT) {
        Ok(source) => source,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let reference_ncs = match load_ncs_bytes("xp1_less25hp.ncs") {
        Ok(ncs) => ncs,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let script = parse_bytes(SourceId::new(3), &source, Some(&langspec))?;

    assert!(!decode_ncs_instructions(&reference_ncs)?.is_empty());
    for bits in 0..=OptimizationFlags::O3.bits() {
        let optimizations = OptimizationFlags::from_bits(bits)
            .ok_or_else(|| test_error(format!("invalid optimization bits {bits:#04x}")))?;
        let compiled_ncs = compile_script(
            &script,
            Some(&langspec),
            CompileOptions {
                optimizations,
                ..CompileOptions::default()
            },
        )?
        .ncs;

        assert_eq!(
            compiled_ncs, reference_ncs,
            "bytecode mismatch for optimization bits {bits:#04x}"
        );

        for (current, maximum) in [(20, 100), (25, 100), (30, 100), (0, 1)] {
            assert_eq!(
                run_less_than_quarter_hit_points(&compiled_ncs, &langspec, current, maximum)?,
                run_less_than_quarter_hit_points(&reference_ncs, &langspec, current, maximum)?,
                "runtime mismatch for optimization bits {bits:#04x}, current={current}, \
                 maximum={maximum}",
            );
        }
    }
    Ok(())
}

#[test]
fn installed_script_compiles_to_parseable_ndb() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let source = match load_nss_bytes(POSITIVE_SCRIPT) {
        Ok(source) => source,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let (source_map, root_id) = {
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file(POSITIVE_SCRIPT.to_string(), source);
        (source_map, root_id)
    };
    let root_source = source_map
        .get(root_id)
        .ok_or_else(|| test_error("missing root source file"))?
        .bytes()
        .to_vec();
    let script = parse_bytes(root_id, &root_source, Some(&langspec))?;
    let artifacts = compile_script_with_source_map(
        &script,
        &source_map,
        root_id,
        Some(&langspec),
        CompileOptions::default(),
    )?;
    let ndb = artifacts
        .ndb
        .ok_or_else(|| test_error("missing NDB output"))?;
    let parsed = read_ndb(&mut std::io::Cursor::new(ndb))?;

    assert!(!parsed.files.is_empty());
    assert!(
        parsed
            .files
            .iter()
            .any(|file| file.is_root && file.name == POSITIVE_SCRIPT)
    );
    Ok(())
}

#[test]
fn installed_negative_scripts_fail_at_the_expected_frontend_boundary() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    for (index, (path, reason)) in NEGATIVE_SCRIPTS.iter().enumerate() {
        let source = match load_nss_bytes(path) {
            Ok(source) => source,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let source_id = SourceId::new(10_000 + u32::try_from(index)?);

        match parse_bytes(source_id, &source, Some(&langspec)) {
            Err(_) => continue,
            Ok(script) => {
                if analyze_script_with_options(
                    &script,
                    Some(&langspec),
                    SemanticOptions {
                        require_entrypoint:       false,
                        allow_conditional_script: false,
                    },
                )
                .is_err()
                {
                    continue;
                }

                let compile_failed = compile_script(
                    &script,
                    Some(&langspec),
                    CompileOptions {
                        semantic: SemanticOptions {
                            require_entrypoint:       false,
                            allow_conditional_script: false,
                        },
                        ..CompileOptions::default()
                    },
                )
                .is_err();

                assert!(
                    compile_failed,
                    "negative script unexpectedly compiled: {path}; {reason}"
                );
            }
        }
    }

    Ok(())
}

#[test]
fn include_backed_script_parses_through_install_resolver() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let resolver = InstallScriptResolver;

    parse_resolved_script(
        &resolver,
        INCLUDE_SCRIPT,
        SourceLoadOptions::default(),
        Some(&langspec),
    )?;
    Ok(())
}

#[test]
fn include_backed_script_semantically_analyzes_through_install_resolver() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let resolver = InstallScriptResolver;
    let script = parse_resolved_script(
        &resolver,
        INCLUDE_SCRIPT,
        SourceLoadOptions::default(),
        Some(&langspec),
    )?;

    analyze_script(&script, Some(&langspec))?;
    Ok(())
}

#[test]
fn include_backed_script_compiles_to_valid_ncs() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let resolver = InstallScriptResolver;
    let script = parse_resolved_script(
        &resolver,
        INCLUDE_SCRIPT,
        SourceLoadOptions::default(),
        Some(&langspec),
    )?;
    let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    assert!(!decode_ncs_instructions(&artifacts.ncs)?.is_empty());
    Ok(())
}

#[test]
fn include_backed_script_compiles_to_parseable_ndb() -> TestResult {
    let langspec = match load_installed_langspec() {
        Ok(value) => value,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let resolver = InstallScriptResolver;
    let bundle = load_source_bundle(&resolver, INCLUDE_SCRIPT, SourceLoadOptions::default())?;
    let artifacts = compile_source_bundle(&bundle, Some(&langspec), CompileOptions::default())?;
    let parsed = read_ndb(&mut std::io::Cursor::new(
        artifacts
            .ndb
            .as_ref()
            .ok_or_else(|| test_error("missing NDB output for include-backed script"))?,
    ))?;

    assert!(parsed.files.len() >= 2);
    assert!(
        parsed
            .files
            .iter()
            .any(|file| file.is_root && file.name == INCLUDE_SCRIPT)
    );
    Ok(())
}
