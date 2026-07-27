use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use nwnrs_types::resman::prelude::ResType;

use super::{
    BatchCompileOptions, BatchCompileStatus, CompileFileOutcome, CompilerDriverOptions,
    CompilerHost, CompilerHostError, FileSystemScriptResolver, GraphvizOutputFormat,
    PendingArtifact, compile_file_with_host, compile_paths, publish_artifacts_transactionally,
    visible_diagnostic_span,
};
use crate::{
    CancellationToken, NW_SCRIPT_SOURCE_RES_TYPE, ScriptResolver, SourceFile, SourceId, Span,
};

#[derive(Default)]
struct MemoryHost {
    sources:  HashMap<(ResType, String), Vec<u8>>,
    files:    Vec<(String, ResType, Vec<u8>)>,
    graphviz: Vec<(String, String)>,
}

impl MemoryHost {
    fn insert_source(&mut self, name: &str, contents: &str) {
        self.sources.insert(
            (NW_SCRIPT_SOURCE_RES_TYPE, name.to_ascii_lowercase()),
            contents.as_bytes().to_vec(),
        );
    }
}

impl CompilerHost for MemoryHost {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, crate::SourceError> {
        Ok(self
            .sources
            .get(&(res_type, script_name.to_ascii_lowercase()))
            .cloned())
    }

    fn resolve_script_bytes_with_cancellation(
        &self,
        script_name: &str,
        res_type: ResType,
        max_bytes: usize,
        cancellation: &CancellationToken,
    ) -> Result<Option<Vec<u8>>, crate::SourceError> {
        cancellation
            .check()
            .map_err(|_error| crate::SourceError::Cancelled)?;
        let Some(bytes) = self
            .sources
            .get(&(res_type, script_name.to_ascii_lowercase()))
        else {
            return Ok(None);
        };
        if bytes.len() > max_bytes {
            return Err(crate::SourceError::resource_limit(
                script_name,
                "memory host source exceeds configured maximum",
            ));
        }
        Ok(Some(bytes.clone()))
    }

    fn write_file(
        &mut self,
        file_name: &str,
        res_type: ResType,
        data: &[u8],
        _binary: bool,
    ) -> Result<(), CompilerHostError> {
        self.files
            .push((file_name.to_string(), res_type, data.to_vec()));
        Ok(())
    }

    fn write_graphviz(&mut self, file_name: &str, dot: &str) -> Result<(), CompilerHostError> {
        self.graphviz.push((file_name.to_string(), dot.to_string()));
        Ok(())
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("nwnrs-types-{prefix}-{nanos}"))
}

#[test]
fn source_diagnostics_never_select_eof_or_whitespace() {
    let source_id = SourceId::new(0);
    let source = "  void main() {\n    Broken();\n}\n\n";
    let file = SourceFile::new(source_id, "broken.nss", source);
    let cases = [
        (Span::new(source_id, source.len(), source.len()), "}"),
        (Span::new(source_id, source.len() - 2, source.len()), "}"),
        (Span::new(source_id, 0, 0), "void"),
        (Span::new(source_id, 0, 2), "void"),
    ];

    for (span, expected) in cases {
        let corrected = visible_diagnostic_span(&file, span);
        assert_eq!(file.span_text(corrected), Some(expected.to_string()));
    }
}

#[test]
fn source_diagnostics_preserve_precise_non_whitespace_spans() {
    let source_id = SourceId::new(0);
    let source = "void main() { Broken(); }";
    let file = SourceFile::new(source_id, "broken.nss", source);
    let start = source.find("Broken").expect("fixture contains symbol");
    let span = Span::new(source_id, start, start + "Broken".len());

    assert_eq!(visible_diagnostic_span(&file, span), span);
}

#[test]
fn compiles_through_callback_host_and_emits_graphviz() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = MemoryHost::default();
    host.insert_source("nwscript", "void PrintInteger(int n);");
    host.insert_source("main", "void main() { PrintInteger(42); }");

    let options = CompilerDriverOptions {
        emit_graphviz: true,
        output_alias: "main".to_string(),
        ..CompilerDriverOptions::default()
    };
    let outcome = compile_file_with_host(&mut host, "main", &options)?;
    assert!(matches!(outcome, CompileFileOutcome::Compiled(_)));
    assert_eq!(host.files.len(), 2);
    assert_eq!(host.graphviz.len(), 1);
    assert_eq!(
        host.graphviz
            .first()
            .map(|(_name, dot)| dot.contains("Function main")),
        Some(true)
    );
    Ok(())
}

#[test]
fn batch_compiler_reports_success_skip_and_error() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("batch");
    fs::create_dir_all(&root)?;
    fs::write(root.join("nwscript.nss"), "void PrintInteger(int n);")?;
    fs::write(root.join("main.nss"), "void main() { PrintInteger(42); }")?;
    fs::write(
        root.join("helper.nss"),
        "int AddOne(int n) { return n + 1; }",
    )?;
    fs::write(root.join("broken.nss"), "void main( {")?;

    let mut options = BatchCompileOptions {
        recurse: true,
        continue_on_error: true,
        simulate: true,
        graphviz_format: GraphvizOutputFormat::Svg,
        keep_graphviz_dot: true,
        driver: CompilerDriverOptions {
            emit_graphviz: true,
            skip_missing_entrypoint: true,
            ..CompilerDriverOptions::default()
        },
        ..BatchCompileOptions::default()
    };
    options.search_roots.push(root.clone());

    let report = compile_paths(std::slice::from_ref(&root), &options)?;
    assert_eq!(report.successes, 1);
    assert_eq!(report.skips, 1);
    assert_eq!(report.errors, 1);
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.status == BatchCompileStatus::Success)
    );
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.status == BatchCompileStatus::Skipped)
    );
    assert!(
        report
            .entries
            .iter()
            .any(|entry| entry.status == BatchCompileStatus::Error)
    );
    let diagnostic = report
        .entries
        .iter()
        .find_map(|entry| entry.diagnostic.as_ref())
        .ok_or("missing structured compiler diagnostic")?;
    assert!(
        diagnostic
            .file
            .as_deref()
            .is_some_and(|file| file.ends_with("broken.nss"))
    );
    assert_eq!(diagnostic.start_line, Some(1));
    assert!(diagnostic.start_column.is_some());
    assert!(diagnostic.end_line.is_some());
    assert!(diagnostic.end_column.is_some());
    let success_outputs = report
        .entries
        .iter()
        .find(|entry| entry.status == BatchCompileStatus::Success)
        .map(|entry| entry.outputs.as_slice())
        .unwrap_or_default();
    assert!(
        success_outputs
            .iter()
            .any(|path| path.extension() == Some("dot".as_ref()))
    );
    assert!(
        success_outputs
            .iter()
            .any(|path| path.extension() == Some("svg".as_ref()))
    );
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn recursive_batch_preserves_output_and_graphviz_hierarchy()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("batch-hierarchy");
    let source_root = root.join("source");
    let nested = source_root.join("chapter/encounters");
    let output_root = root.join("compiled");
    let graphviz_root = root.join("graphs");
    fs::create_dir_all(&nested)?;
    fs::write(
        source_root.join("nwscript.nss"),
        "void PrintInteger(int n);",
    )?;
    fs::write(
        nested.join("ambush.nss"),
        "void main() { PrintInteger(42); }",
    )?;

    let options = BatchCompileOptions {
        recurse: true,
        output_directory: Some(output_root.clone()),
        graphviz_directory: Some(graphviz_root.clone()),
        driver: CompilerDriverOptions {
            emit_graphviz: true,
            ..CompilerDriverOptions::default()
        },
        ..BatchCompileOptions::default()
    };
    let report = compile_paths(std::slice::from_ref(&source_root), &options)?;

    assert_eq!(report.successes, 1);
    assert!(output_root.join("chapter/encounters/ambush.ncs").is_file());
    assert!(
        graphviz_root
            .join("chapter/encounters/ambush.dot")
            .is_file()
    );
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn filesystem_resolver_checks_roots_and_default_extension() -> Result<(), Box<dyn std::error::Error>>
{
    let root = unique_temp_dir("resolver");
    fs::create_dir_all(&root)?;
    fs::write(root.join("test.nss"), "void main() {}")?;
    let resolver = FileSystemScriptResolver::with_root(&root).expect("add resolver root");
    let resolved = resolver.resolve_script_bytes("test", NW_SCRIPT_SOURCE_RES_TYPE)?;
    assert!(resolved.is_some());
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn filesystem_resolver_matches_script_names_case_insensitively()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("resolver-case");
    fs::create_dir_all(&root)?;
    fs::write(root.join("MixedCase.NSS"), "void main() {}")?;
    let resolver = FileSystemScriptResolver::with_root(&root).expect("add resolver root");
    let resolved = resolver.resolve_script_bytes("mixedcase.nss", NW_SCRIPT_SOURCE_RES_TYPE)?;
    assert!(resolved.is_some());
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn filesystem_resolver_never_falls_back_outside_configured_roots()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("resolver-scope");
    let source = root.join("source");
    fs::create_dir_all(&source)?;
    fs::write(root.join("outside.nss"), "void Outside() {}")?;
    let resolver = FileSystemScriptResolver::with_root(&source).expect("add resolver root");

    assert!(
        resolver
            .resolve_script_bytes("../outside.nss", NW_SCRIPT_SOURCE_RES_TYPE)?
            .is_none()
    );
    assert!(
        resolver
            .resolve_script_bytes(
                &crate::physical_path_display(&root.join("outside.nss")),
                NW_SCRIPT_SOURCE_RES_TYPE,
            )?
            .is_none()
    );
    let mut resolver = resolver;
    let error = resolver
        .add_overlay(root.join("outside.nss"), b"void Overlay() {}".to_vec())
        .expect_err("reject an overlay outside configured roots");
    assert!(
        error
            .to_string()
            .contains("outside every configured script root")
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn filesystem_resolver_rejects_symlink_escapes() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let root = unique_temp_dir("resolver-symlink-scope");
    let source = root.join("source");
    fs::create_dir_all(&source)?;
    fs::write(root.join("outside.nss"), "void Outside() {}")?;
    symlink(root.join("outside.nss"), source.join("escaped.nss"))?;
    let resolver = FileSystemScriptResolver::with_root(&source).expect("add resolver root");

    assert!(
        resolver
            .resolve_script_bytes("escaped.nss", NW_SCRIPT_SOURCE_RES_TYPE)?
            .is_none()
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn filesystem_resolver_rejects_ambiguous_case_ties() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("resolver-case-tie");
    fs::create_dir_all(&root)?;
    fs::write(root.join("Alpha.nss"), "first")?;
    fs::write(root.join("alpha.NSS"), "second")?;
    let resolver = FileSystemScriptResolver::with_root(&root).expect("add resolver root");

    let error = resolver
        .resolve_script_bytes("Alpha.nss", NW_SCRIPT_SOURCE_RES_TYPE)
        .expect_err("ambiguous case-insensitive matches must fail");
    assert!(error.to_string().contains("ambiguous case-insensitive"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn batch_compilation_stops_before_work_when_cancelled() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("cancelled-batch");
    fs::create_dir_all(&root)?;
    let source = root.join("main.nss");
    fs::write(&source, "void main() {}")?;
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let options = BatchCompileOptions {
        cancellation: Some(cancellation),
        simulate: true,
        ..BatchCompileOptions::default()
    };
    let error = compile_paths(&[source], &options).expect_err("cancelled batch must fail");
    assert_eq!(error.to_string(), "operation cancelled");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn filesystem_resolver_enforces_size_before_returning_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("resolver-limit");
    fs::create_dir_all(&root)?;
    fs::write(root.join("large.nss"), b"12345678")?;
    let resolver = FileSystemScriptResolver::with_root(&root).expect("add resolver root");
    let cancellation = CancellationToken::new();

    let error = resolver
        .resolve_script_bytes_with_cancellation(
            "large",
            NW_SCRIPT_SOURCE_RES_TYPE,
            4,
            &cancellation,
        )
        .expect_err("oversized source must fail before it is returned");
    assert!(error.to_string().contains("configured maximum 4"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn filesystem_resolver_observes_precancelled_reads() -> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("resolver-cancel");
    fs::create_dir_all(&root)?;
    fs::write(root.join("main.nss"), b"void main() {}")?;
    let resolver = FileSystemScriptResolver::with_root(&root).expect("add resolver root");
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = resolver
        .resolve_script_bytes_with_cancellation(
            "main",
            NW_SCRIPT_SOURCE_RES_TYPE,
            1024,
            &cancellation,
        )
        .expect_err("cancelled source read must fail");
    assert_eq!(error.to_string(), "operation cancelled");
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn output_transaction_rejects_case_insensitive_collisions() -> Result<(), Box<dyn std::error::Error>>
{
    let root = unique_temp_dir("output-case-collision");
    fs::create_dir_all(&root)?;
    let artifacts = vec![
        PendingArtifact::Output(super::PendingOutput {
            path: root.join("Main.ncs"),
            data: Some(vec![1]),
        }),
        PendingArtifact::Output(super::PendingOutput {
            path: root.join("main.NCS"),
            data: Some(vec![2]),
        }),
    ];

    let error = publish_artifacts_transactionally(artifacts, true, None)
        .expect_err("ambiguous output targets must fail atomically");
    assert!(error.to_string().contains("ambiguous case-insensitive"));
    assert!(!root.join("Main.ncs").exists());
    assert!(!root.join("main.NCS").exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn output_transaction_observes_cancellation_before_rendering()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("output-cancel");
    fs::create_dir_all(&root)?;
    let target = root.join("main.svg");
    let artifacts = vec![PendingArtifact::Graphviz {
        dot:           "digraph test {}".to_string(),
        dot_path:      None,
        rendered_path: target.clone(),
        format:        GraphvizOutputFormat::Svg,
    }];
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = publish_artifacts_transactionally(artifacts, true, Some(&cancellation))
        .expect_err("cancelled output transaction must not start Graphviz");
    assert_eq!(error.to_string(), "operation cancelled");
    assert!(!target.exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn no_debug_batch_removes_stale_ndb_in_the_artifact_transaction()
-> Result<(), Box<dyn std::error::Error>> {
    let root = unique_temp_dir("stale-debug-transaction");
    fs::create_dir_all(&root)?;
    fs::write(root.join("nwscript.nss"), "int TRUE = 1;\nint FALSE = 0;\n")?;
    fs::write(root.join("main.nss"), "void main() {}")?;
    fs::write(root.join("main.ndb"), b"stale debug data")?;
    let mut options = BatchCompileOptions {
        remove_stale_debug: true,
        ..BatchCompileOptions::default()
    };
    options.driver.session.emit_debug = false;

    let report = compile_paths(&[root.join("main.nss")], &options)?;
    assert_eq!(report.successes, 1);
    assert!(root.join("main.ncs").is_file());
    assert!(!root.join("main.ndb").try_exists()?);
    fs::remove_dir_all(root)?;
    Ok(())
}
