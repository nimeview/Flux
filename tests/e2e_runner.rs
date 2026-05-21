use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn runtime_bin() -> &'static str {
    static BIN: OnceLock<String> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Ok(bin) = std::env::var("FLUX_RUNTIME_BIN") { return bin; }
        let status = Command::new("cmake").args(["-S", "runtime", "-B", "runtime/build"]).status().expect("cmake configure failed");
        assert!(status.success(), "cmake configure failed");
        let status = Command::new("cmake").args(["--build", "runtime/build"]).status().expect("cmake build failed");
        assert!(status.success(), "cmake build failed");
        "runtime/build/runtime".to_string()
    })
}

fn tmpdir() -> PathBuf {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("flux_e2e_{n}"));
    fs::create_dir_all(&d).unwrap();
    d
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for ent in fs::read_dir(src).unwrap() {
        let ent = ent.unwrap();
        let p = ent.path();
        let target = dst.join(ent.file_name());
        if p.is_dir() { copy_tree(&p, &target); } else { fs::copy(&p, &target).unwrap(); }
    }
}

fn match_with_placeholders(expected: &str, actual: &str) -> bool {
    let e: Vec<_> = expected.lines().collect();
    let a: Vec<_> = actual.lines().collect();
    if e.len() != a.len() { return false; }
    e.iter().zip(a.iter()).all(|(ex, ac)| {
        if *ex == "{{number}}" { return ac.parse::<f64>().is_ok(); }
        ex == ac
    })
}

fn run_case(path: &str, trace: bool, expect_compile_fail: bool) {
    let src = PathBuf::from(path);
    let case_dir = src.parent().unwrap();
    let name = src.file_stem().unwrap().to_string_lossy().to_string();
    let temp = tmpdir();
    copy_tree(case_dir, &temp);
    let local_src = temp.join(src.file_name().unwrap());
    let out = temp.join(format!("{name}.fxbc"));

    let c = Command::new(env!("CARGO_BIN_EXE_Flux")).arg(&local_src).arg(&out).output().unwrap();
    if expect_compile_fail {
        assert!(!c.status.success(), "expected compile failure");
        let stderr = String::from_utf8_lossy(&c.stderr);
        let golden = fs::read_to_string(src.with_extension("stderr")).unwrap();
        assert!(stderr.contains(golden.trim()), "compile stderr mismatch\nexpected contains: {golden}\nactual: {stderr}");
        return;
    }
    assert!(c.status.success(), "compiler failed: {}", String::from_utf8_lossy(&c.stderr));

    let mut cmd = Command::new(runtime_bin());
    cmd.arg(&out);
    if trace { cmd.arg("--trace"); }
    let r = cmd.output().unwrap();
    assert!(r.status.success(), "runtime failed: {}", String::from_utf8_lossy(&r.stderr));

    let stdout = String::from_utf8_lossy(&r.stdout).replace("\r\n", "\n");
    let stderr = String::from_utf8_lossy(&r.stderr).replace("\r\n", "\n");

    if let Ok(golden) = fs::read_to_string(src.with_extension("stdout")) {
        assert!(match_with_placeholders(golden.trim_end(), stdout.trim_end()), "stdout mismatch\nexpected:\n{golden}\nactual:\n{stdout}");
    }
    if let Ok(golden) = fs::read_to_string(src.with_extension("stderr")) {
        assert!(stderr.contains(golden.trim()), "stderr mismatch\nexpected contains:\n{golden}\nactual:\n{stderr}");
    }
    if let Ok(golden) = fs::read_to_string(src.with_extension("trace")) {
        for line in golden.lines().filter(|l| !l.trim().is_empty()) {
            assert!(stderr.contains(line), "trace line missing: {line}\nactual:\n{stderr}");
        }
    }
}

#[test]
fn e2e_smoke_reactive_chain() { run_case("tests/e2e/smoke/reactive_chain.fx", true, false); }
#[test]
fn e2e_smoke_import_aliases() { run_case("tests/e2e/smoke/import_aliases.fx", false, false); }
#[test]
fn e2e_smoke_stdlib_math_time_vec() { run_case("tests/e2e/smoke/stdlib_math_time_vec.fx", false, false); }
#[test]
fn e2e_smoke_if_match() { run_case("tests/e2e/smoke/if_match.fx", false, false); }
#[test]
fn e2e_smoke_error_diagnostics() { run_case("tests/e2e/smoke/error_diagnostics.fx", false, true); }
#[test]
fn e2e_regression_alias_collision() { run_case("tests/e2e/regression/regression_alias_collision.fx", false, true); }

#[test]
#[ignore = "stress"]
fn e2e_stress_large_graph() { run_case("tests/e2e/stress/large_graph.fx", false, false); }
#[test]
#[ignore = "stress"]
fn e2e_stress_many_sets_one_tick() { run_case("tests/e2e/stress/many_sets_one_tick.fx", true, false); }
