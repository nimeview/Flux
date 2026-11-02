//! End-to-end reference programs. Each directory under `tests/golden` is a
//! program, and each `cases/<name>` holds its `args` (one per line), optional
//! `stdin`, and the expected `stdout`, `stderr`, and `status`. Every case runs
//! on the VM and the JIT, from a private copy, and must match exactly.
//!
//! `FLUX_BLESS=1 cargo test -p flux_cli --test golden` rewrites the expected
//! files from the VM's output; review the diff before committing it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn programs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(golden_dir())
        .expect("golden directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let (path, name) = (entry.path(), entry.file_name());
        if name == "cases" || name == ".flux" {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &to.join(name));
        } else {
            fs::copy(&path, to.join(name)).unwrap();
        }
    }
}

struct Outcome {
    stdout: String,
    stderr: String,
    status: String,
}

fn run_case(program: &Path, case: &Path, engine: &str) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(program, dir.path());
    let mut args = vec![
        "run".to_string(),
        engine.to_string(),
        "--no-cache".to_string(),
    ];
    args.push("--quiet".to_string());
    if dir.path().join("flux.toml").exists() {
        args.push("--locked".to_string());
    } else {
        args.push("main.fl".to_string());
    }
    let program_args = fs::read_to_string(case.join("args")).unwrap_or_default();
    if !program_args.is_empty() {
        args.push("--".to_string());
        args.extend(program_args.lines().map(str::to_string));
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(&args)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flux");
    let stdin = fs::read(case.join("stdin")).unwrap_or_default();
    child.stdin.take().unwrap().write_all(&stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let clean = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .replace(root.to_str().unwrap(), "<dir>")
            .replace(dir.path().to_str().unwrap(), "<dir>")
    };
    Outcome {
        stdout: clean(&output.stdout),
        stderr: clean(&output.stderr),
        status: format!("{:?}\n", output.status.code()),
    }
}

fn cases(program: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(program.join("cases"))
        .expect("a program needs cases")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn golden_programs_match_on_both_engines() {
    let bless = std::env::var_os("FLUX_BLESS").is_some();
    let mut failures = Vec::new();
    for program in programs() {
        let mut saw_success = false;
        let mut saw_failure = false;
        for case in cases(&program) {
            let name = format!(
                "{}/{}",
                program.file_name().unwrap().to_string_lossy(),
                case.file_name().unwrap().to_string_lossy()
            );
            if bless {
                let vm = run_case(&program, &case, "--vm");
                fs::write(case.join("stdout"), &vm.stdout).unwrap();
                fs::write(case.join("stderr"), &vm.stderr).unwrap();
                fs::write(case.join("status"), &vm.status).unwrap();
            }
            let want_status = fs::read_to_string(case.join("status")).unwrap_or_default();
            saw_success |= want_status == "Some(0)\n";
            saw_failure |= want_status != "Some(0)\n";
            for engine in ["--vm", "--jit"] {
                let got = run_case(&program, &case, engine);
                for (what, actual) in [
                    ("stdout", &got.stdout),
                    ("stderr", &got.stderr),
                    ("status", &got.status),
                ] {
                    let want = fs::read_to_string(case.join(what)).unwrap_or_default();
                    if &want != actual {
                        failures.push(format!(
                            "{name} {engine} {what}\n  want: {want:?}\n  got:  {actual:?}"
                        ));
                    }
                }
            }
        }
        assert!(
            saw_success && saw_failure,
            "{}: needs a success case and a failure case",
            program.display()
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn flux_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            flux_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "fl") {
            out.push(path);
        }
    }
}

#[test]
fn golden_programs_are_formatted() {
    let mut files = Vec::new();
    flux_files(&golden_dir(), &mut files);
    assert!(!files.is_empty());
    for file in files {
        let source = fs::read_to_string(&file).unwrap();
        let formatted = flux_fmt::format_source(&source);
        assert_eq!(
            formatted.as_deref().ok(),
            Some(source.as_str()),
            "{} is not formatted",
            file.display()
        );
    }
}

/// The repository's examples are what a new user tries first.
#[test]
fn examples_compile_and_are_formatted() {
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let mut files = Vec::new();
    flux_files(&examples, &mut files);
    assert!(!files.is_empty());
    for file in files {
        let source = fs::read_to_string(&file).unwrap();
        let formatted = flux_fmt::format_source(&source);
        assert_eq!(
            formatted.as_deref().ok(),
            Some(source.as_str()),
            "{} is not formatted",
            file.display()
        );
        let relative = file.strip_prefix(&examples).unwrap();
        // `errors/` shows diagnostics; a package is checked from its own
        // directory by `packages_example.rs`.
        if relative.starts_with("packages") {
            continue;
        }
        let compiled = flux_cli::pipeline::check_file(&file);
        assert_eq!(
            compiled.is_err(),
            relative.starts_with("errors"),
            "{}",
            file.display()
        );
    }
}
