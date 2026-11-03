use std::process::Command;

#[test]
fn generic_callback_result_and_struct_accumulator_run_on_both_engines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.fl"),
        r#"
import io { println }
import collections { reduce }
struct Acc { v: Int }
fn apply[T, U](x: T, f: fn(T) -> U) -> U { f(x) }
fn main() {
    println(apply("a", |i| i))
    println(apply(3, |i| i + 1))
    println(reduce([1, 2], Acc { v: 0 }, |a, b| Acc { v: a.v + b }).v)
}
"#,
    )
    .unwrap();
    let mut outputs = Vec::new();
    for engine in ["--vm", "--jit"] {
        let output = Command::new(env!("CARGO_BIN_EXE_flux"))
            .args(["run", engine, "--no-cache", "main.fl"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "a\n4\n3\n");
        outputs.push(output);
    }
    assert_eq!(outputs[0].stdout, outputs[1].stdout);
    assert_eq!(outputs[0].stderr, outputs[1].stderr);
}
