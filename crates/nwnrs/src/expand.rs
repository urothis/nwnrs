use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use nwnrs_nwscript as nwscript;
use tracing::instrument;

use crate::{
    args::ExpandCmd,
    compile::{CliCompilerHost, build_install_resman},
    util::ensure_output_file_ready,
};

pub(crate) struct ExpandedScript {
    pub(crate) source: String,
    pub(crate) trace:  Vec<nwscript::MacroExpansionTrace>,
    source_map:        nwscript::SourceMap,
}

pub(crate) fn expand_script(cmd: &ExpandCmd) -> Result<ExpandedScript, String> {
    if !(1..=200).contains(&cmd.max_include_depth) {
        return Err("maximum include depth must be between 1 and 200".to_string());
    }
    if !cmd.input.is_file() {
        return Err(format!(
            "input source does not exist: {}",
            cmd.input.display()
        ));
    }

    let mut include_dirs = cmd.include_dir.clone();
    for dependency in nwnrs_nwpkg::resolve_include_dependencies(&cmd.input)? {
        if !include_dirs.contains(&dependency.source_root) {
            include_dirs.push(dependency.source_root);
        }
    }
    let install_resman = build_install_resman(
        cmd.root.as_deref(),
        cmd.user.as_deref(),
        &cmd.language,
        cmd.load_ovr,
    )?;
    let host = CliCompilerHost::for_source(&cmd.input, &include_dirs, install_resman);
    let source_load = nwscript::SourceLoadOptions {
        max_include_depth: cmd.max_include_depth,
        ..nwscript::SourceLoadOptions::default()
    };
    let bundle = nwscript::load_source_bundle(&host, &cmd.input.to_string_lossy(), source_load)
        .map_err(|error| format!("failed to load {}: {error}", cmd.input.display()))?;
    let mut registry = nwscript::MacroRegistry::new();
    let (preprocessed, trace) = nwscript::preprocess_source_bundle_with_macro_trace(
        &bundle,
        &mut registry,
        nwscript::MacroExpansionOptions::default(),
    )
    .map_err(|error| format!("failed to expand {}: {error}", cmd.input.display()))?;
    let stream = nwscript::NwTokenStream::from_tokens(&preprocessed.tokens)
        .map_err(|error| format!("expanded source is not balanced: {error}"))?;
    let mut source = nwscript::render_nwscript_tokens(&stream);
    if !source.ends_with('\n') {
        source.push('\n');
    }
    Ok(ExpandedScript {
        source,
        trace,
        source_map: bundle.source_map,
    })
}

#[instrument(level = "info", skip_all, err, fields(input = %cmd.input.display()))]
pub(crate) fn run_expand(cmd: ExpandCmd) -> Result<(), String> {
    if let Some(output) = cmd.output.as_deref() {
        reject_source_output_alias(&cmd.input, output)?;
        ensure_output_file_ready(output, cmd.force)?;
    }
    let expanded = expand_script(&cmd)?;
    if cmd.trace_macros {
        write_trace(&expanded)?;
    }
    if let Some(output) = cmd.output.as_deref() {
        fs::write(output, expanded.source.as_bytes())
            .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    } else {
        let mut stdout = io::stdout().lock();
        stdout
            .write_all(expanded.source.as_bytes())
            .map_err(|error| format!("failed to write stdout: {error}"))?;
    }
    Ok(())
}

fn write_trace(expanded: &ExpandedScript) -> Result<(), String> {
    let mut stderr = io::stderr().lock();
    for event in &expanded.trace {
        let location = expanded
            .source_map
            .get(event.span.source_id)
            .and_then(|file| {
                file.location(event.span.start)
                    .map(|location| format!("{}:{}:{}", file.name, location.line, location.column))
            })
            .unwrap_or_else(|| {
                format!("source#{}:{}", event.span.source_id.get(), event.span.start)
            });
        let input = nwscript::render_nwscript_tokens(&event.input);
        let output = nwscript::render_nwscript_tokens(&event.output);
        writeln!(
            stderr,
            "[macro depth={}] {location} {}!({input})\n=> {output}",
            event.depth, event.path
        )
        .map_err(|error| format!("failed to write macro trace: {error}"))?;
    }
    Ok(())
}

fn reject_source_output_alias(input: &Path, output: &Path) -> Result<(), String> {
    if input == output {
        return Err("expanded output would overwrite its source file".to_string());
    }
    if output.exists()
        && let (Ok(input), Ok(output)) = (fs::canonicalize(input), fs::canonicalize(output))
        && input == output
    {
        return Err("expanded output would overwrite its source file".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{expand_script, run_expand};
    use crate::args::ExpandCmd;

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-{prefix}-{nanos}"))
    }

    fn expand_cmd(input: PathBuf) -> ExpandCmd {
        ExpandCmd {
            force: false,
            trace_macros: false,
            output: None,
            include_dir: Vec::new(),
            max_include_depth: 16,
            root: None,
            user: None,
            language: "english".to_string(),
            load_ovr: false,
            input,
        }
    }

    #[test]
    fn expands_includes_declarative_macros_and_records_trace() {
        let temp_dir = unique_test_dir("expand");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        fs::write(
            temp_dir.join("helper.nss"),
            "macro_rules! value { () => { 42 }; }\n",
        )
        .expect("write include");
        let input = temp_dir.join("main.nss");
        fs::write(
            &input,
            "#include \"helper\"\nvoid main() { int value = value!(); }\n",
        )
        .expect("write source");

        let expanded = expand_script(&expand_cmd(input)).expect("expand source");
        assert!(expanded.source.contains("42"));
        assert!(!expanded.source.contains("macro_rules"));
        let [trace] = expanded.trace.as_slice() else {
            panic!("expected exactly one trace record");
        };
        assert_eq!(trace.path.to_string(), "value");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn expands_local_nwpkg_include_dependencies() {
        let temp_dir = unique_test_dir("expand-nwpkg");
        let project = temp_dir.join("project");
        let include = temp_dir.join("include");
        fs::create_dir_all(&project).expect("create project dir");
        fs::create_dir_all(&include).expect("create include dir");
        fs::write(
            project.join("nwproject.toml"),
            "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \
             \".\"\n\n[dependencies]\nfixture = { path = \"../include\" }\n",
        )
        .expect("write project manifest");
        fs::write(
            include.join("nwproject.toml"),
            "[project]\nname = \"fixture\"\nkind = \"include\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write include manifest");
        fs::write(
            include.join("fixture.nss"),
            "macro_rules! dependency_value { () => { 23 }; }\n",
        )
        .expect("write dependency include");
        let input = project.join("main.nss");
        fs::write(
            &input,
            "#include \"fixture\"\nvoid main() { int value = dependency_value!(); }\n",
        )
        .expect("write project source");

        let expanded = expand_script(&expand_cmd(input)).expect("expand nwpkg dependency");
        assert!(expanded.source.contains("23"));
        assert!(!expanded.source.contains("dependency_value!"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn output_requires_force_and_never_overwrites_the_source() {
        let temp_dir = unique_test_dir("expand-output");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("main.nss");
        fs::write(&input, "void main() {}\n").expect("write source");
        let output = temp_dir.join("expanded.nss");
        fs::write(&output, "existing\n").expect("write output");

        let mut command = expand_cmd(input.clone());
        command.output = Some(output.clone());
        assert!(run_expand(command.clone()).is_err());
        command.force = true;
        run_expand(command).expect("force output");
        assert!(
            fs::read_to_string(&output)
                .expect("read output")
                .contains("main")
        );

        let mut destructive = expand_cmd(input.clone());
        destructive.output = Some(input);
        destructive.force = true;
        assert!(run_expand(destructive).is_err());

        let _ = fs::remove_dir_all(temp_dir);
    }
}
