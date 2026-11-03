//! `flux test`: runs every `fn test_*()` in the project's `.fl` files.
//!
//! A test passes when it returns (`Unit` or `true`) without a runtime error;
//! a failed `assert`/`assert_eq`, any other runtime error, or `false` fails
//! it. Each test runs in a fresh executor, so one failure cannot leak into
//! another, and a file's `main` is never run.

use std::path::{Path, PathBuf};

use flux_runtime::{ExecutionMode, Executor, RuntimeValue};

use crate::output::report_failure;
use crate::pipeline::{compile_file_with_options, CompileOptions};

/// Outcome counts for the summary line.
#[derive(Default)]
struct Tally {
    passed: usize,
    failed: usize,
}

/// Runs the tests under `root` whose `path::name` contains `filter` and
/// returns the process exit status (1 if anything failed).
pub fn run_tests(root: &Path, filter: Option<&str>, mode: ExecutionMode) -> i32 {
    let mut files = Vec::new();
    collect_fl_files(root, &mut files);
    files.sort();

    let mut tally = Tally::default();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        if !text.contains("fn test_") {
            continue;
        }
        let display = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        run_file_tests(&file, &display, filter, mode, &mut tally);
    }

    if tally.passed + tally.failed == 0 {
        println!("no tests found");
        return 0;
    }
    println!(
        "\ntest result: {}. {} passed; {} failed",
        if tally.failed == 0 { "ok" } else { "FAILED" },
        tally.passed,
        tally.failed
    );
    i32::from(tally.failed > 0)
}

fn run_file_tests(
    file: &Path,
    display: &str,
    filter: Option<&str>,
    mode: ExecutionMode,
    tally: &mut Tally,
) {
    let compiled = match compile_file_with_options(file, CompileOptions::run(mode)) {
        Ok(compiled) => compiled,
        Err(failed) => {
            if filter.is_none_or(|f| display.contains(f)) {
                println!("test {display} ... FAILED: does not compile");
                let _ = report_failure(failed.source(), &failed.sources, &failed.diagnostics);
                tally.failed += 1;
            }
            return;
        }
    };
    // Root-module test functions only: imported modules' names are mangled.
    let mut tests: Vec<String> = compiled
        .hir
        .names
        .values()
        .filter(|name| name.starts_with("test_"))
        .filter(|name| compiled.executor.function_arity(name).is_some())
        .cloned()
        .collect();
    tests.sort();
    for name in tests {
        let id = format!("{display}::{name}");
        if filter.is_some_and(|f| !id.contains(f)) {
            continue;
        }
        let outcome = run_one(&compiled.executor.program, &name, mode);
        match outcome {
            Ok(()) => {
                println!("test {id} ... ok");
                tally.passed += 1;
            }
            Err(reason) => {
                println!("test {id} ... FAILED: {reason}");
                tally.failed += 1;
            }
        }
    }
}

fn run_one(
    program: &flux_ir::CompiledProgram,
    name: &str,
    mode: ExecutionMode,
) -> Result<(), String> {
    let mut executor = Executor::with_mode(program.clone(), mode).map_err(|e| e.to_string())?;
    if executor.function_arity(name) != Some(0) {
        return Err(format!("`{name}` must take no parameters"));
    }
    match executor.call(name, &[]) {
        Ok(RuntimeValue::Bool(false)) => Err("returned false".into()),
        Ok(_) => Ok(()),
        Err(err) => Err(format!("{err:#}")),
    }
}

/// `.fl` files under `dir`, skipping hidden directories and `target`.
fn collect_fl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !name.starts_with('.') && name != "target" {
                collect_fl_files(&path, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("fl") {
            out.push(path);
        }
    }
}
