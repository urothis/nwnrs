#![cfg(feature = "tooling")]
//! Integration coverage for the command-line JSON diagnostic stream.

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn unique_test_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("nwnrs-compile-json-{nanos}"))
}

#[test]
fn compile_json_reports_machine_readable_source_diagnostics() {
    let root = unique_test_dir();
    fs::create_dir_all(&root).expect("create fixture directory");
    let langspec = root.join("nwscript.nss");
    let source = root.join("broken.nss");
    fs::write(
        &langspec,
        b"#define ENGINE_NUM_STRUCTURES 0\nint TRUE = 1;\nint FALSE = 0;\n",
    )
    .expect("write fixture langspec");
    fs::write(
        &source,
        b"void main()\n{\n    int value = missing_identifier;\n}\n",
    )
    .expect("write broken script");

    let output = Command::new(env!("CARGO_BIN_EXE_nwnrs"))
        .args([
            "compile",
            "--simulate",
            "--continue-on-error",
            "--diagnostic-format",
            "json",
            "--langspec",
        ])
        .arg(&langspec)
        .arg(&source)
        .output()
        .expect("run compiler");

    let _ = fs::remove_dir_all(&root);
    assert!(!output.status.success());
    let records = String::from_utf8(output.stdout)
        .expect("compiler stdout is UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("valid JSON line"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);

    let diagnostic = records.first().expect("diagnostic record");
    assert_eq!(diagnostic["kind"], "diagnostic");
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["code"], -622);
    assert_eq!(diagnostic["start_line"], 3);
    assert_eq!(diagnostic["start_column"], 17);
    assert!(
        diagnostic["file"]
            .as_str()
            .is_some_and(|file| file.ends_with("broken.nss"))
    );

    let summary = records.get(1).expect("summary record");
    assert_eq!(summary["kind"], "summary");
    assert_eq!(summary["compiled"], 0);
    assert_eq!(summary["failed"], 1);
    assert_eq!(summary["simulated"], true);
}
