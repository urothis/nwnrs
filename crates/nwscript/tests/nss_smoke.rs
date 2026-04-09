#![allow(missing_docs)]

use std::{error::Error, path::PathBuf};

use nwnrs_nwscript::prelude::*;

mod support;

use support::{
    assets_root, load_nss_bytes, skip_if_remote_assets_unavailable, test_error,
};

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
    (
        "x2_inc_banter.nss",
        "expected failure: unexpected backtick character in source",
    ),
    (
        "x0_inc_skills.nss",
        "expected failure: declaration and implementation return types differ",
    ),
];

#[derive(Debug, Clone)]
struct RemoteScriptResolver {
    assets: PathBuf,
}

impl RemoteScriptResolver {
    fn new(assets: PathBuf) -> Self {
        Self { assets }
    }
}

impl ScriptResolver for RemoteScriptResolver {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: nwnrs_restype::prelude::ResType,
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
            match load_nss_bytes(&self.assets, &candidate) {
                Ok(bytes) => return Ok(Some(bytes)),
                Err(_) => continue,
            }
        }

        Ok(None)
    }
}

fn load_langspec_remote() -> Result<(std::path::PathBuf, LangSpec), Box<dyn Error>> {
    let assets = assets_root();
    let langspec_source = load_nss_bytes(&assets, "nwscript.nss")?;
    let langspec = parse_langspec_bytes("nwscript.nss", &langspec_source)?;
    Ok((assets, langspec))
}

#[test]
fn smoke_remote_script_parses_and_analyzes() -> TestResult {
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let source = match load_nss_bytes(&assets, POSITIVE_SCRIPT) {
        Ok(source) => source,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let script = parse_bytes(SourceId::new(1), &source, Some(&langspec))?;
    analyze_script(&script, Some(&langspec))?;
    Ok(())
}

#[test]
fn smoke_remote_script_compiles_to_deterministic_ncs() -> TestResult {
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let source = match load_nss_bytes(&assets, POSITIVE_SCRIPT) {
        Ok(source) => source,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let script = parse_bytes(SourceId::new(2), &source, Some(&langspec))?;

    let first = compile_script(&script, Some(&langspec), CompileOptions::default())?;
    let second = compile_script(&script, Some(&langspec), CompileOptions::default())?;

    assert_eq!(first.ncs, second.ncs);
    assert!(!decode_ncs_instructions(&first.ncs)?.is_empty());
    Ok(())
}

#[test]
fn smoke_remote_script_compiles_to_parseable_ndb() -> TestResult {
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let source = match load_nss_bytes(&assets, POSITIVE_SCRIPT) {
        Ok(source) => source,
        Err(error) => return skip_if_remote_assets_unavailable(error),
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
    assert!(parsed.files.iter().any(|file| file.is_root && file.name == POSITIVE_SCRIPT));
    Ok(())
}

#[test]
fn negative_remote_scripts_fail_at_the_current_frontend_boundary() -> TestResult {
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };

    for (index, (path, _reason)) in NEGATIVE_SCRIPTS.iter().enumerate() {
        let source = match load_nss_bytes(&assets, path) {
            Ok(source) => source,
            Err(error) => return skip_if_remote_assets_unavailable(error),
        };
        let source_id = SourceId::new(10_000 + u32::try_from(index)?);

        match parse_bytes(source_id, &source, Some(&langspec)) {
            Err(_) => continue,
            Ok(script) => {
                if analyze_script_with_options(
                    &script,
                    Some(&langspec),
                    SemanticOptions {
                        require_entrypoint: false,
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
                            require_entrypoint: false,
                            allow_conditional_script: false,
                        },
                        ..CompileOptions::default()
                    },
                )
                .is_err();

                assert!(compile_failed, "negative script unexpectedly compiled: {path}");
            }
        }
    }

    Ok(())
}

#[test]
fn include_backed_script_parses_through_remote_resolver() -> TestResult {
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let resolver = RemoteScriptResolver::new(assets);

    parse_resolved_script(
        &resolver,
        INCLUDE_SCRIPT,
        SourceLoadOptions::default(),
        Some(&langspec),
    )?;
    Ok(())
}

#[test]
fn include_backed_script_semantically_analyzes_through_remote_resolver() -> TestResult {
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let resolver = RemoteScriptResolver::new(assets);
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
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let resolver = RemoteScriptResolver::new(assets);
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
    let (assets, langspec) = match load_langspec_remote() {
        Ok(value) => value,
        Err(error) => return skip_if_remote_assets_unavailable(error),
    };
    let resolver = RemoteScriptResolver::new(assets);
    let bundle = load_source_bundle(&resolver, INCLUDE_SCRIPT, SourceLoadOptions::default())?;
    let artifacts = compile_source_bundle(&bundle, Some(&langspec), CompileOptions::default())?;
    let parsed = read_ndb(
        &mut std::io::Cursor::new(
            artifacts
                .ndb
                .as_ref()
                .ok_or_else(|| test_error("missing NDB output for include-backed script"))?,
        ),
    )?;

    assert!(parsed.files.len() >= 2);
    assert!(parsed.files.iter().any(|file| file.is_root && file.name == INCLUDE_SCRIPT));
    Ok(())
}
