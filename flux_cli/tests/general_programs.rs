//! Phase 8: ordinary, non-reactive programs in `examples/general` run end to
//! end and print the expected report on both engines.

use std::path::PathBuf;
use std::process::Command;

fn run(program: &str, engine: &str) -> String {
    run_with(program, engine, &[])
}

fn run_with(program: &str, engine: &str, extra: &[&str]) -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/general");
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["run", engine, program])
        .args(extra)
        .current_dir(&dir)
        .output()
        .expect("spawn flux");
    assert!(
        output.status.success(),
        "{program} {engine}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// With `fn main`, stdout is exactly the program's report (no binding dump).
fn assert_report(program: &str, expected: &[&str]) {
    for engine in ["--vm", "--jit"] {
        let stdout = run(program, engine);
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines, expected, "{program} {engine}");
    }
}

#[test]
fn text_stats_counts_lines_words_and_blanks() {
    assert_report(
        "text_stats.fl",
        &["lines: 4", "words: 11", "blank: 1", "longest: 23"],
    );
}

#[test]
fn inventory_totals_parses_and_aggregates_records() {
    assert_report(
        "inventory_totals.fl",
        &[
            "apple: 3 x 0.5 = 1.5",
            "pear: 2 x 0.75 = 1.5",
            "fig: 10 x 0.2 = 2",
            "items: 15, total: 5",
        ],
    );
}

#[test]
fn config_check_validates_required_keys() {
    assert_report("config_check.fl", &["entries: 3", "ok: api on port 8080"]);
}

#[test]
fn text_stats_reads_the_file_named_on_the_command_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("two.txt");
    std::fs::write(&file, "one two\nthree\n").unwrap();
    let stdout = run_with("text_stats.fl", "--vm", &["--", file.to_str().unwrap()]);
    assert!(stdout.starts_with("lines: 2\nwords: 3\n"), "{stdout}");
}
