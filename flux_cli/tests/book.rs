//! Every example in the book (`docs/flux/src`) and the README runs, so that they
//! cannot drift from the language.
//!
//! A ```` ```flux ```` block is a whole program. It must exit 0 on the VM and the
//! JIT with the same output and no warnings, and be formatted. A ```` ```output ````
//! or ```` ```stderr ```` block right after it is the exact expected text
//! (`<dir>/` stands for the example's directory). Attributes after the language:
//!
//! - `exit=N`: the expected exit status instead of 0.
//! - `error=E005`: must fail to compile, with that code.
//! - `file=name.fl`: not run; written for the chapter's later examples to import.
//! - `fragment`: not run; only for pieces that are not programs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn book_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/flux/src")
}

struct Example {
    line: usize,
    attrs: Vec<String>,
    code: String,
    stdout: Option<String>,
    stderr: Option<String>,
}

fn examples(text: &str) -> Vec<Example> {
    let mut found: Vec<Example> = Vec::new();
    let mut lines = text.lines().enumerate().peekable();
    while let Some((number, line)) = lines.next() {
        let Some(info) = line.strip_prefix("```") else {
            continue;
        };
        let mut body = String::new();
        for (_, inner) in lines.by_ref() {
            if inner.starts_with("```") {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        let mut parts = info.split(',').map(str::trim);
        match parts.next() {
            Some("flux") => found.push(Example {
                line: number + 1,
                attrs: parts.map(str::to_string).collect(),
                code: body,
                stdout: None,
                stderr: None,
            }),
            Some("output") => found.last_mut().expect("output after flux").stdout = Some(body),
            Some("stderr") => found.last_mut().expect("stderr after flux").stderr = Some(body),
            _ => {}
        }
    }
    found
}

fn run(dir: &Path, engine: &str, file: &str) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["run", engine, "--no-cache", "--quiet", file])
        .current_dir(dir)
        .output()
        .expect("spawn flux");
    let root = dir.canonicalize().unwrap();
    let clean = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .replace(root.to_str().unwrap(), "<dir>")
            .replace(dir.to_str().unwrap(), "<dir>")
    };
    (
        output.status.code(),
        clean(&output.stdout),
        clean(&output.stderr),
    )
}

fn check(chapter: &str, dir: &Path, example: &Example, failures: &mut Vec<String>) {
    let at = format!("{chapter}:{}", example.line);
    let attr = |name: &str| {
        example
            .attrs
            .iter()
            .find_map(|a| a.strip_prefix(name)?.strip_prefix('='))
    };
    if example.attrs.iter().any(|a| a == "fragment") {
        return;
    }
    if let Some(name) = attr("file") {
        fs::write(dir.join(name), &example.code).unwrap();
        return;
    }
    fs::write(dir.join("main.fl"), &example.code).unwrap();
    let vm = run(dir, "--vm", "main.fl");
    let jit = run(dir, "--jit", "main.fl");
    if vm != jit {
        failures.push(format!("{at}: engines differ\nvm: {vm:?}\njit: {jit:?}"));
    }
    if let Some(code) = attr("error") {
        if vm.0 != Some(1) || !vm.2.contains(&format!("flux::{code}")) {
            failures.push(format!("{at}: expected {code}, got {vm:?}"));
        }
        return;
    }
    let want_status = attr("exit").map_or(0, |n| n.parse().unwrap());
    if vm.0 != Some(want_status) {
        failures.push(format!("{at}: status {:?}\n{}", vm.0, vm.2));
    }
    if let Some(want) = &example.stdout {
        if &vm.1 != want {
            failures.push(format!("{at}: stdout\nwant: {want:?}\ngot:  {:?}", vm.1));
        }
    }
    match &example.stderr {
        Some(want) if &vm.2 != want => {
            failures.push(format!("{at}: stderr\nwant: {want:?}\ngot:  {:?}", vm.2));
        }
        None if !vm.2.is_empty() => failures.push(format!("{at}: unexpected stderr\n{}", vm.2)),
        _ => {}
    }
    let fmt = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["fmt", "--check", "main.fl"])
        .current_dir(dir)
        .output()
        .expect("spawn flux");
    if !fmt.status.success() {
        failures.push(format!("{at}: not formatted"));
    }
}

#[test]
fn every_book_example_runs_on_both_engines() {
    let mut failures = Vec::new();
    let mut count = 0;
    let mut chapters: Vec<_> = fs::read_dir(book_dir())
        .expect("book sources")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "md"))
        .collect();
    chapters.sort();
    chapters.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../README.md"));
    for path in chapters {
        let chapter = path.file_name().unwrap().to_string_lossy().into_owned();
        let dir = tempfile::tempdir().unwrap();
        for example in examples(&fs::read_to_string(&path).unwrap()) {
            count += 1;
            check(&chapter, dir.path(), &example, &mut failures);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert!(count > 30, "only {count} examples found");
}
