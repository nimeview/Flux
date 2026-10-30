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
