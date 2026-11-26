//! The host model: timers and the event loop (`time`), `extern fn`, and
//! the library each target has. Every program runs on both engines, which
//! must agree.

use std::path::Path;
use std::process::Command;

fn flux(dir: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn flux");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn both(src: &str) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    let vm = flux(
        dir.path(),
        &["run", "--vm", "--no-cache", "--quiet", "main.fl"],
    );
    let jit = flux(
        dir.path(),
        &["run", "--jit", "--no-cache", "--quiet", "main.fl"],
    );
    assert_eq!(vm, jit, "{src}");
    vm
}

fn prints(src: &str, want: &str) {
    let (status, stdout, stderr) = both(src);
    assert_eq!(status, Some(0), "{src}\n{stderr}");
    assert_eq!(stdout, want, "{src}");
}

fn check(src: &str, target: &str) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    flux(dir.path(), &["check", "--target", target, "main.fl"])
}

/// After `main`, the program runs while timers are set or until `stop()`;
/// each firing is one batch, and timers due together fire in the order set.
#[test]
fn timers_run_after_main() {
    prints(
        r#"import io { println }
import time { after, every, now, stop, cancel }

event Tick
event Say(text: String)
event Late

let mut n = 0

on Tick {
    n += 1
    println("tick {n}")
    if n == 3 {
        stop()
    }
}

on Say(text) {
    println(text)
}

on Late {
    println("never")
}

fn main() {
    let start = now()
    every(20, Tick())
    after(10, Say("first"))
    after(10, Say("second"))
    let t = after(5, Late())
    cancel(t)
    println("main done {now() - start < 1000}")
}
"#,
        "main done true\nfirst\nsecond\ntick 1\ntick 2\ntick 3\n",
    );
}

/// A component's timer for its own event goes to that instance.
#[test]
fn a_component_sets_timers_for_itself() {
    prints(
        r#"import io { println }
import time { after }

live fn Blinker(name: String, times: Int) {
    pub let mut left = times
    pub event Blink

    on Blink {
        println("{name} blink")
        left -= 1
        if left > 0 {
            after(5, Blink())
        }
    }
}

event Start

let a = Blinker("a", 2)
let b = Blinker("b", 3)

on Start {
    emit a.Blink()
    emit b.Blink()
}

fn main() {
    emit Start()
}
"#,
        "a blink\nb blink\na blink\nb blink\nb blink\n",
    );
}

#[test]
fn timers_are_actions() {
    let (status, _, stderr) =
        both("import time { after }\nevent T\nderived d = after(5, T())\nfn main() {\n}\n");
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("side effect `after` is not allowed in a derived expression"),
        "{stderr}"
    );
    let (status, _, stderr) = both("import time { after }\nfn main() {\n    after(5, 3)\n}\n");
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("an event to send, as in `after(500, Tick())`"),
        "{stderr}"
    );
}

/// `extern fn` is declared in Flux and given by the host; the command line
/// gives none, so a program that declares one does not start there.
#[test]
fn extern_functions_need_a_host() {
    let (status, _, stderr) = both("extern fn ask(q: String) -> Int\n\nfn main() {\n}\n");
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("this host does not provide `ask`, declared with `extern fn`"),
        "{stderr}"
    );
    // It type-checks, and calling one is an action.
    let (status, stdout, _) = check(
        "extern fn ask(q: String) -> Int\n\nfn main() {\n    let n = ask(\"x\")\n}\n",
        "cli",
    );
    assert_eq!((status, stdout.as_str()), (Some(0), "ok\n"));
    let (status, _, stderr) = check(
        "extern fn ask(q: String) -> Int\n\nderived d = ask(\"x\")\n\nfn main() {\n}\n",
        "cli",
    );
    assert_eq!(status, Some(1));
    assert!(stderr.contains("E008"), "{stderr}");
}

/// Each target has its part of the library: `web` has no files,
/// processes, environment, standard input, or arguments.
#[test]
fn targets_have_their_library() {
    let (status, _, stderr) = check("import fs { list_dir }\n\nfn main() {\n}\n", "web");
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("`fs` is not available on target `web`") && stderr.contains("E034"),
        "{stderr}"
    );
    let (status, _, stderr) = check("import io { read_line }\n\nfn main() {\n}\n", "web");
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("`read_line` is not available on target `web`"),
        "{stderr}"
    );
    let (status, _, stderr) = check(
        "import io\n\nfn main() {\n    let x = read_line()\n}\n",
        "web",
    );
    assert_eq!(status, Some(1));
    assert!(
        stderr.contains("`read_line` is not available on target `web`"),
        "{stderr}"
    );
    let ok = "import io\nimport time { now }\n\nfn main() {\n    println(\"{now()}\")\n}\n";
    assert_eq!(check(ok, "web").1, "ok\n");
    assert_eq!(check("import fs\n\nfn main() {\n}\n", "cli").1, "ok\n");
    let (status, _, stderr) = check(ok, "mars");
    assert_eq!(status, Some(1));
    assert!(stderr.contains("unknown target `mars`"), "{stderr}");
}
