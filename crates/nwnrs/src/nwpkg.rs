use std::{env, path::PathBuf};

use nwnrs_nwpkg::{
    DependencyOperation, DependencyOperationKind, DependencyResolverConfiguration,
    clean_dependency_cache, plan_dependency_operation, resolve_dependency_operation,
    write_registered_dependency_lock,
};
use nwnrs_nwscript::CancellationToken;

use crate::{
    args::{NwpkgCleanCmd, NwpkgFetchCmd, NwpkgUpdateCmd},
    util::write_stdout_line,
};

pub(crate) fn run_fetch(command: NwpkgFetchCmd) -> Result<(), String> {
    let root = package_root(command.path)?;
    let configuration = configuration(command.git, command.cache);
    let result = resolve_dependency_operation(
        &root,
        &configuration,
        DependencyOperationKind::Fetch,
        command.dependency.as_deref(),
        &CancellationToken::new(),
    )
    .map_err(String::from)?;
    print_result("fetched", &result)
}

pub(crate) fn run_update(command: NwpkgUpdateCmd) -> Result<(), String> {
    let root = package_root(command.path)?;
    let configuration = configuration(command.git, command.cache);
    let result = plan_dependency_operation(
        &root,
        &configuration,
        DependencyOperationKind::Update,
        command.dependency.as_deref(),
        &CancellationToken::new(),
    )
    .map_err(String::from)?;
    print_result("update plan", &result)?;
    if command.apply {
        write_registered_dependency_lock(&configuration, &root, &result.lock)
            .map_err(String::from)?;
        write_stdout_line("applied nwpkg.lock update")?;
    } else {
        write_stdout_line("preview only; rerun with --apply to replace nwpkg.lock")?;
    }
    Ok(())
}

pub(crate) fn run_clean(command: NwpkgCleanCmd) -> Result<(), String> {
    let _root = package_root(command.path)?;
    let mut configuration = DependencyResolverConfiguration::from_environment();
    if let Some(cache) = command.cache {
        configuration.cache_root = cache;
    }
    let removed = clean_dependency_cache(&configuration).map_err(String::from)?;
    for path in &removed {
        write_stdout_line(&format!("removed {}", path.display()))?;
    }
    write_stdout_line(&format!(
        "removed {} unused dependency cache entr{}",
        removed.len(),
        if removed.len() == 1 { "y" } else { "ies" }
    ))
}

fn print_result(label: &str, result: &DependencyOperation) -> Result<(), String> {
    if result.changes.is_empty() {
        write_stdout_line(&format!("{label}: no commit changes"))?;
    } else {
        for (route, (before, after)) in &result.changes {
            write_stdout_line(&format!(
                "{label}: {route}: {} -> {after}",
                before.as_deref().unwrap_or("<unlocked>")
            ))?;
        }
    }
    write_stdout_line(&format!(
        "resolved {} dependency package{}",
        result.lock.packages.len(),
        if result.lock.packages.len() == 1 {
            ""
        } else {
            "s"
        }
    ))
}

fn configuration(git: Option<PathBuf>, cache: Option<PathBuf>) -> DependencyResolverConfiguration {
    let mut configuration = DependencyResolverConfiguration::from_environment();
    if let Some(git) = git {
        configuration.git_executable = git;
    }
    if let Some(cache) = cache {
        configuration.cache_root = cache;
    }
    configuration
}

fn package_root(path: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = match path {
        Some(path) => path,
        None => env::current_dir()
            .map_err(|error| format!("failed to determine current directory: {error}"))?,
    };
    if path.is_dir() {
        Ok(path)
    } else {
        path.parent()
            .map(PathBuf::from)
            .ok_or_else(|| format!("package path has no parent: {}", path.display()))
    }
}
