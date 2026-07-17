use std::{error::Error, io};

use nwnrs_types::{
    mdl::{
        MODEL_RES_TYPE, ModelDiagnosticKind, ModelEncoding, ParsedModel, compile_ascii_model,
        compile_semantic_model, detect_model_encoding, lower_ascii_model, lower_binary_model,
        lower_semantic_model_to_scene, parse_binary_model_bytes, parse_model_bytes,
        write_original_binary_model,
    },
    test_support::{
        read_resource_bytes, shipped_resource_candidates, skip_if_game_resources_unavailable,
    },
};

const GAME_CORPUS_SAMPLE_LIMIT: usize = 512;

#[test]
fn shipped_game_mdl_sample_parses_lowers_and_roundtrips() -> Result<(), Box<dyn Error>> {
    let candidates = match shipped_resource_candidates(MODEL_RES_TYPE) {
        Ok(candidates) => candidates,
        Err(error) => return skip_if_game_resources_unavailable(Box::new(error)),
    };
    if candidates.is_empty() {
        return Err(test_error("NWN installation exposes no MDL resources"));
    }

    let selected = evenly_spaced_indices(candidates.len(), GAME_CORPUS_SAMPLE_LIMIT);
    let mut compiled_count = 0_usize;
    let mut scene_count = 0_usize;
    let mut semantic_recompile_count = 0_usize;
    let mut failures = Vec::new();

    for index in selected {
        let Some(candidate) = candidates.get(index) else {
            continue;
        };
        let name = candidate.to_file();
        let bytes = match read_resource_bytes(candidate.base().res_ref(), MODEL_RES_TYPE) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{name}: read failed: {error}"));
                continue;
            }
        };

        let result = match detect_model_encoding(&bytes) {
            ModelEncoding::Ascii => exercise_ascii_model(&bytes).map(|coverage| {
                scene_count += usize::from(coverage.scene);
                semantic_recompile_count += usize::from(coverage.recompiled);
            }),
            ModelEncoding::Compiled => {
                compiled_count += 1;
                exercise_compiled_model(&bytes).map(|coverage| {
                    scene_count += usize::from(coverage.scene);
                    semantic_recompile_count += usize::from(coverage.recompiled);
                })
            }
        };
        if let Err(error) = result {
            failures.push(format!("{name}: {error}"));
        }
    }

    if compiled_count == 0 {
        failures.push("game corpus sample contained no compiled MDLs".to_string());
    }
    if scene_count == 0 {
        failures.push("no sampled game MDL lowered to a scene".to_string());
    }
    if semantic_recompile_count == 0 {
        failures.push("no sampled game MDL passed strict semantic recompilation".to_string());
    }
    if !failures.is_empty() {
        return Err(test_error(failures.join("\n")));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ExerciseCoverage {
    scene:      bool,
    recompiled: bool,
}

fn exercise_ascii_model(bytes: &[u8]) -> Result<ExerciseCoverage, Box<dyn Error>> {
    let ParsedModel::Ascii(ascii) = parse_model_bytes(bytes)? else {
        return Err(test_error(
            "ASCII corpus entry was detected as compiled MDL",
        ));
    };
    let semantic = lower_ascii_model(&ascii)?;
    lower_semantic_model_to_scene(&semantic)?;
    let compiled = compile_ascii_model(&ascii)?;
    let reparsed = parse_binary_model_bytes(compiled.original_bytes())?;
    lower_binary_model(&reparsed)?;
    Ok(ExerciseCoverage {
        scene:      true,
        recompiled: true,
    })
}

fn exercise_compiled_model(bytes: &[u8]) -> Result<ExerciseCoverage, Box<dyn Error>> {
    let binary = parse_binary_model_bytes(bytes)?;
    let mut restored = Vec::new();
    write_original_binary_model(&mut restored, &binary)?;
    if restored != bytes {
        return Err(test_error(
            "binary writer did not preserve the source bytes",
        ));
    }

    let semantic = lower_binary_model(&binary)?;
    let scene = lower_semantic_model_to_scene(&semantic).is_ok();
    if let Some(diagnostic) = semantic
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == ModelDiagnosticKind::UnsupportedValue)
    {
        return Err(test_error(format!(
            "unsupported compiled data was not preserved: {}",
            diagnostic.message
        )));
    }
    let malformed_source = semantic.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.kind,
            ModelDiagnosticKind::MalformedValue | ModelDiagnosticKind::MalformedPayloadRow
        )
    });
    if malformed_source {
        // The source-byte writer above must still preserve malformed shipped
        // assets exactly. A semantic compiler must not invent replacement data
        // for an out-of-range pointer or truncated payload.
        return Ok(ExerciseCoverage {
            scene,
            recompiled: false,
        });
    }
    let recompiled = compile_semantic_model(&semantic)?;
    parse_binary_model_bytes(recompiled.original_bytes())?;
    Ok(ExerciseCoverage {
        scene,
        recompiled: true,
    })
}

fn evenly_spaced_indices(length: usize, limit: usize) -> Vec<usize> {
    if length <= limit {
        return (0..length).collect();
    }
    (0..limit)
        .map(|index| index.saturating_mul(length) / limit)
        .collect()
}

fn test_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::other(message.into()))
}
