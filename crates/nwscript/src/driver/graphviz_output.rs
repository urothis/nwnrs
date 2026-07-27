use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use super::{
    BatchCompileError, CompilerHostError, GraphvizOutputFormat, PendingArtifact,
    output::{PendingOutput, publish_outputs_transactionally, validate_output_paths},
};

pub(super) fn publish_artifacts_transactionally(
    artifacts: Vec<PendingArtifact>,
    overwrite_existing: bool,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<(), CompilerHostError> {
    let mut paths = Vec::with_capacity(artifacts.len().saturating_mul(2));
    for artifact in &artifacts {
        match artifact {
            PendingArtifact::Output(output) => paths.push(output.path.as_path()),
            PendingArtifact::Removal(_) => {}
            PendingArtifact::Graphviz {
                dot_path,
                rendered_path,
                ..
            } => {
                if let Some(dot_path) = dot_path {
                    paths.push(dot_path.as_path());
                }
                paths.push(rendered_path.as_path());
            }
        }
    }
    validate_output_paths(paths, overwrite_existing)?;
    let mut outputs = Vec::with_capacity(artifacts.len().saturating_mul(2));
    for artifact in artifacts {
        if let Some(cancellation) = cancellation {
            cancellation
                .check()
                .map_err(|error| CompilerHostError::new(error.to_string()))?;
        }
        match artifact {
            PendingArtifact::Output(output) => outputs.push(output),
            PendingArtifact::Removal(path) => outputs.push(PendingOutput {
                path,
                data: None,
            }),
            PendingArtifact::Graphviz {
                dot,
                dot_path,
                rendered_path,
                format,
            } => {
                let rendered = render_graphviz(&dot, format, cancellation)?;
                if let Some(dot_path) = dot_path {
                    outputs.push(PendingOutput {
                        path: dot_path,
                        data: Some(dot.into_bytes()),
                    });
                }
                outputs.push(PendingOutput {
                    path: rendered_path,
                    data: Some(rendered),
                });
            }
        }
    }
    publish_outputs_transactionally(outputs, overwrite_existing, cancellation)
}

fn render_graphviz(
    dot: &str,
    format: GraphvizOutputFormat,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<Vec<u8>, CompilerHostError> {
    let mut child = Command::new("dot")
        .arg(format!("-T{}", format.extension()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CompilerHostError::new(format!(
                "failed to start Graphviz 'dot'; install Graphviz or request DOT output: {error}"
            ))
        })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| CompilerHostError::new("failed to open Graphviz stdin"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| CompilerHostError::new("failed to open Graphviz stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| CompilerHostError::new("failed to open Graphviz stderr"))?;
    let dot = dot.as_bytes().to_vec();
    let input_thread = thread::spawn(move || stdin.write_all(&dot));
    let output_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let error_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });

    let status = loop {
        if let Some(cancellation) = cancellation
            && let Err(cancelled) = cancellation.check()
        {
            let mut cleanup_errors = Vec::new();
            if let Err(error) = child.kill()
                && error.kind() != io::ErrorKind::InvalidInput
            {
                cleanup_errors.push(format!("failed to terminate Graphviz: {error}"));
            }
            if let Err(error) = child.wait() {
                cleanup_errors.push(format!("failed to reap Graphviz: {error}"));
            }
            collect_graphviz_thread_error(input_thread, "input", &mut cleanup_errors);
            collect_graphviz_thread_error(output_thread, "output", &mut cleanup_errors);
            collect_graphviz_thread_error(error_thread, "error", &mut cleanup_errors);
            let suffix = if cleanup_errors.is_empty() {
                String::new()
            } else {
                format!(
                    "; Graphviz cleanup also failed: {}",
                    cleanup_errors.join("; ")
                )
            };
            return Err(CompilerHostError::new(format!("{cancelled}{suffix}")));
        }
        match child.try_wait()? {
            Some(status) => break status,
            None => thread::sleep(Duration::from_millis(10)),
        }
    };

    join_graphviz_thread(input_thread, "input")?;
    let stdout = join_graphviz_thread(output_thread, "output")?;
    let stderr = join_graphviz_thread(error_thread, "error")?;
    if !status.success() {
        let detail = escaped_process_output(&stderr);
        return Err(CompilerHostError::new(format!(
            "Graphviz rendering failed for {} output: {}",
            format.extension(),
            detail.trim()
        )));
    }
    Ok(stdout)
}

fn join_graphviz_thread<T>(
    thread: thread::JoinHandle<io::Result<T>>,
    stream: &str,
) -> Result<T, CompilerHostError> {
    thread
        .join()
        .map_err(|_panic_payload| {
            CompilerHostError::new(format!("Graphviz {stream} worker panicked"))
        })?
        .map_err(|error| {
            CompilerHostError::new(format!("failed to process Graphviz {stream}: {error}"))
        })
}

fn collect_graphviz_thread_error<T>(
    thread: thread::JoinHandle<io::Result<T>>,
    stream: &str,
    errors: &mut Vec<String>,
) {
    match thread.join() {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => errors.push(format!("failed to process Graphviz {stream}: {error}")),
        Err(_) => errors.push(format!("Graphviz {stream} worker panicked")),
    }
}

pub(super) fn ensure_graphviz_renderer(
    format: GraphvizOutputFormat,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<(), BatchCompileError> {
    if !format.requires_renderer() {
        return Ok(());
    }
    let mut child = Command::new("dot")
        .arg("-V")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            BatchCompileError::Configuration(format!(
                "Graphviz image output requires the 'dot' executable; install Graphviz or select \
                 DOT output: {error}"
            ))
        })?;
    let status = loop {
        if let Some(cancellation) = cancellation
            && let Err(cancelled) = cancellation.check()
        {
            let kill = child.kill();
            let wait = child.wait();
            let mut cleanup = Vec::new();
            if let Err(error) = kill
                && error.kind() != io::ErrorKind::InvalidInput
            {
                cleanup.push(format!("failed to terminate Graphviz probe: {error}"));
            }
            if let Err(error) = wait {
                cleanup.push(format!("failed to reap Graphviz probe: {error}"));
            }
            return if cleanup.is_empty() {
                Err(BatchCompileError::Cancelled(cancelled))
            } else {
                Err(BatchCompileError::Configuration(format!(
                    "{cancelled}; Graphviz probe cleanup also failed: {}",
                    cleanup.join("; ")
                )))
            };
        }
        match child.try_wait()? {
            Some(status) => break status,
            None => thread::sleep(Duration::from_millis(10)),
        }
    };
    let mut detail = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        stderr.read_to_end(&mut detail)?;
    }
    if status.success() {
        Ok(())
    } else {
        let detail = escaped_process_output(&detail);
        Err(BatchCompileError::Configuration(format!(
            "Graphviz 'dot -V' returned an unsuccessful status: {}",
            detail.trim()
        )))
    }
}

fn escaped_process_output(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(value) => value.to_string(),
        Err(_) => bytes
            .iter()
            .flat_map(|byte| std::ascii::escape_default(*byte))
            .map(char::from)
            .collect(),
    }
}
