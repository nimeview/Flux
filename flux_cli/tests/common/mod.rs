//! Shared helpers for `programs/` integration tests.
#![allow(dead_code)] // each helper is used by a different integration test binary

use std::fs;
use std::path::{Path, PathBuf};

use flux_cli::pipeline::{compile, compile_file, source_from};
use flux_driver::check_file;
use flux_runtime::ExecutionMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestMode {
    /// Full compile (and run when the test executes the executor).
    Run,
    /// Typecheck only — skips IR/executor (stdin, blocking init I/O).
    CheckOnly,
    /// Valid module used only as a dependency; skip fail-suite roots.
    Stub,
}

pub fn programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs")
}

/// Reads `// test: check-only | stub | run` from the file header.
pub fn parse_test_mode(contents: &str) -> TestMode {
    for line in contents.lines().take(12) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("// test:") {
            return match rest.trim() {
                "check-only" => TestMode::CheckOnly,
                "stub" => TestMode::Stub,
                "run" => TestMode::Run,
                other => panic!("unknown // test: directive: {other}"),
            };
        }
        if !line.starts_with("//") && !line.is_empty() {
            break;
        }
    }
    TestMode::Run
}

pub fn display_path(path: &Path) -> String {
    path.strip_prefix(programs_dir())
        .unwrap_or(path)
        .display()
        .to_string()
}

pub fn read_program(relative: &str) -> (String, String) {
    let path = programs_dir().join(relative);
    let contents = fs::read_to_string(&path).expect("read program");
    (display_path(&path), contents)
}

pub fn collect_fl_files(category: &str) -> Vec<PathBuf> {
    fn walk(base: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(base).unwrap_or_else(|e| panic!("read {}: {e}", base.display())) {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "fl") {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    walk(&programs_dir().join(category), &mut files);
    files.sort();
    files
}

/// Compile (or check) a program file, honoring `// test:` directives.
pub fn compile_program_path(path: &Path) {
    let contents = fs::read_to_string(path).expect("read program");
    let mode = parse_test_mode(&contents);
    let label = display_path(path);

    match mode {
        TestMode::CheckOnly => {
            check_file(path).unwrap_or_else(|e| {
                panic!("{label} check-only failed: {:?}", e.diagnostics.errors);
            });
        }
        TestMode::Stub => panic!("{label} is a stub and must not be compiled directly"),
        TestMode::Run => {
            if contents.contains("import ") {
                compile_file(path, ExecutionMode::Vm).unwrap_or_else(|e| {
                    panic!("{label} compile failed: {:?}", e.diagnostics.errors);
                });
            } else {
                let source = source_from(label.clone(), contents);
                compile(source).unwrap_or_else(|e| {
                    panic!("{label} compile failed: {:?}", e.diagnostics.errors);
                });
            }
        }
    }
}

/// Whether a `programs/fail` file is a regression entry (not a dependency stub).
pub fn is_fail_regression_root(path: &Path, contents: &str) -> bool {
    if parse_test_mode(contents) == TestMode::Stub {
        return false;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    // Legacy fallback until every stub carries `// test: stub`.
    if name.ends_with("_stub.fl") || name == "cyclic_a.fl" || name == "cyclic_b.fl" {
        return false;
    }
    true
}
