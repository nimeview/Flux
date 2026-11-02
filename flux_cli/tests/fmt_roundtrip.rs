//! `flux fmt` must never change what a program means: every valid program in
//! the repository still parses and type-checks after formatting, and
//! formatting is idempotent.

use std::fs;
use std::path::{Path, PathBuf};

use flux_cli::pipeline::check_file;
use flux_fmt::format_source;

fn program_dirs() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut dirs = Vec::new();
    for base in ["programs/ok", "programs/demo", "examples"] {
        collect_dirs(&root.join(base), &mut dirs);
    }
    dirs
}

fn collect_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    out.push(dir.to_path_buf());
    for entry in fs::read_dir(dir).unwrap().flatten() {
        if entry.path().is_dir() {
            collect_dirs(&entry.path(), out);
        }
    }
}

#[test]
fn formatted_programs_still_type_check() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for dir in program_dirs() {
        // A package with dependencies cannot resolve from a copied directory;
        // `packages_example.rs` checks those files with `flux fmt --check`.
        let manifest = std::fs::read_to_string(dir.join("flux.toml")).unwrap_or_default();
        if manifest.contains("[dependencies]") {
            continue;
        }
        // Format a whole directory together so sibling imports still resolve.
        let copy = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("fl") {
                continue;
            }
            let source = fs::read_to_string(&path).unwrap();
            let target = copy.path().join(path.file_name().unwrap());
            match format_source(&source) {
                Ok(formatted) => {
                    let again = format_source(&formatted).expect("formatted output parses");
                    if again != formatted {
                        failures.push(format!("{}: not idempotent", path.display()));
                    }
                    fs::write(&target, formatted).unwrap();
                    files.push((path, target));
                }
                // Parse errors and comments inside items are refused, never mangled.
                Err(_) => fs::write(&target, source).unwrap(),
            }
        }
        for (original, formatted) in files {
            if check_file(&original).is_err() {
                continue; // only programs that were valid before formatting
            }
            checked += 1;
            if let Err(failure) = check_file(&formatted) {
                failures.push(format!(
                    "{}: {:?}",
                    original.display(),
                    failure.diagnostics.errors
                ));
            }
        }
    }
    assert!(checked > 40, "only {checked} programs checked");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The syntax tree of `source` without its source positions.
fn tree_without_spans(source: &str) -> Option<String> {
    let mut diagnostics = flux_diagnostics::DiagnosticBag::new();
    let program =
        flux_syntax::parse_program(flux_diagnostics::SourceId(0), source, &mut diagnostics)?;
    let tree = format!("{program:?}");
    let mut out = String::with_capacity(tree.len());
    let mut rest = tree.as_str();
    while let Some(at) = rest.find("Span {") {
        out.push_str(&rest[..at]);
        let end = rest[at..].find('}').expect("span ends");
        rest = &rest[at + end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// Formatting used to drop every parenthesis, turning `(1 + 2) * 3` into
/// `1 + 2 * 3`, and only type-checking was tested.
#[test]
fn formatting_never_changes_a_programs_tree() {
    let mut checked = 0;
    let mut failures = Vec::new();
    let mut files = Vec::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for dir in program_dirs()
        .into_iter()
        .chain([root.join("tests/golden"), root.join("benches/workloads")])
    {
        let mut dirs = vec![dir.clone()];
        collect_dirs(&dir, &mut dirs);
        for dir in dirs {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                if entry.path().extension().is_some_and(|ext| ext == "fl") {
                    files.push(entry.path());
                }
            }
        }
    }
    files.sort();
    files.dedup();
    for file in files {
        let source = fs::read_to_string(&file).unwrap();
        let Some(before) = tree_without_spans(&source) else {
            continue;
        };
        let formatted = format_source(&source).expect("parsed source formats");
        let after = tree_without_spans(&formatted)
            .unwrap_or_else(|| panic!("{}: output does not parse", file.display()));
        if before != after {
            let at = before
                .bytes()
                .zip(after.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let window = |text: &str| {
                let start = at.saturating_sub(70);
                let end = (at + 90).min(text.len());
                text.get(start..end).unwrap_or("").to_string()
            };
            failures.push(format!(
                "{}\n  before: {}\n  after:  {}",
                file.display(),
                window(&before),
                window(&after)
            ));
        }
        checked += 1;
    }
    assert!(checked > 100, "only {checked} programs checked");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn formatting_keeps_exactly_the_parentheses_that_matter() {
    for (input, want) in [
        ("(1 + 2) * 3", "(1 + 2) * 3"),
        ("1 + (2 * 3)", "1 + 2 * 3"),
        ("1 - (2 - 3)", "1 - (2 - 3)"),
        ("(1 - 2) - 3", "1 - 2 - 3"),
        ("(1 + 2) % 3", "(1 + 2) % 3"),
        ("-(1 + 2)", "-(1 + 2)"),
        ("!(a && b)", "!(a && b)"),
        ("(a || b) && c", "(a || b) && c"),
        ("a || (b && c)", "a || b && c"),
        ("(a == b) == c", "a == b == c"),
        ("a == (b == c)", "a == (b == c)"),
        ("(x as Int) + 1", "x as Int + 1"),
        ("(1 + 2) as Float", "(1 + 2) as Float"),
        ("-(x as Float)", "-(x as Float)"),
        ("(-x) as Float", "-x as Float"),
        ("(1 + 2).f", "(1 + 2).f"),
        ("(a + b)[0]", "(a + b)[0]"),
        ("(a + b)?", "(a + b)?"),
        ("(-a).f", "(-a).f"),
        ("(f)(1)", "f(1)"),
        ("(|x| x)(1)", "(|x| x)(1)"),
        ("(|x| x) + 1", "(|x| x) + 1"),
        ("(if a { 1 } else { 2 }) + 1", "(if a { 1 } else { 2 }) + 1"),
        ("1 + if a { 1 } else { 2 }", "1 + if a { 1 } else { 2 }"),
        ("(-9223372036854775808)[0]", "(-9223372036854775808)[0]"),
        ("(-1).f", "(-1).f"),
    ] {
        let formatted = format_source(&format!("const q = {input}\n")).expect("formats");
        assert_eq!(formatted, format!("const q = {want}\n"), "{input}");
    }
}

#[test]
fn formatting_keeps_suffixed_literal_patterns_valid() {
    for pattern in ["1i8", "2u16", "3.5f32", "-1", "1"] {
        let source = format!("const q = match x {{\n    {pattern} => 2,\n    _ => 3\n}}\n");
        let once = format_source(&source).expect("formats");
        assert!(once.contains(pattern), "{once}");
        assert_eq!(format_source(&once).expect("output parses"), once);
    }
}
