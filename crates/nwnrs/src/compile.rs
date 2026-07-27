#[cfg(test)]
pub(crate) use nwnrs_language::DEFAULT_OPTIMIZATION;
pub(crate) use nwnrs_language::{
    CompileScriptOptions, CompileScriptOutcome, LanguageCompilerHost, autodetected_install_resman,
    build_install_resman, build_install_script_resolver, compile_generated_script,
    compile_script_file, compile_script_file_with_skip, parse_optimizations,
};

use crate::args::CompileCmd;

pub(crate) type CliCompilerHost = LanguageCompilerHost;

pub(crate) fn run_compile(command: CompileCmd) -> Result<(), String> {
    nwnrs_language::run_compile(nwnrs_language::CompileCommandOptions {
        force:               command.force,
        debug:               command.debug,
        no_entrypoint_check: command.no_entrypoint_check,
        langspec:            command.langspec,
        include_dir:         command.include_dir,
        optimization:        command.optimization,
        optimization_flag:   command.optimization_flag,
        max_include_depth:   command.max_include_depth,
        graphviz:            command.graphviz,
        graphviz_format:     command.graphviz_format,
        keep_graphviz_dot:   command.keep_graphviz_dot,
        simulate:            command.simulate,
        diagnostic_format:   command.diagnostic_format,
        continue_on_error:   command.continue_on_error,
        recurse:             command.recurse,
        follow_symlinks:     command.follow_symlinks,
        jobs:                command.jobs,
        output:              command.output,
        directory:           command.directory,
        root:                command.root,
        user:                command.user,
        language:            command.language,
        load_ovr:            command.load_ovr,
        paths:               command.paths,
    })
    .map_err(|error| error.message)
}
