//! `flux repl` driven through stdin: sessions keep their state, `:emit`
//! takes the event's values, runtime errors are shown, and the end of input
//! ends the session.

use std::io::Write;
use std::process::{Command, Stdio};

/// Runs a REPL session on `input`; returns its exit status and output.
fn repl(input: &str) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_flux"))
        .arg("repl")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flux repl");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    // Without `:quit`: the end of input ends the session, or this waits.
    let out = child.wait_with_output().expect("wait for flux repl");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_new_declaration_keeps_the_sessions_state() {
    let (status, stdout, _) = repl(
        "event Inc\nlet mut count = 0\non Inc {\n    count += 1\n}\n:emit Inc\nlet marker = 9\n",
    );
    assert_eq!(status, Some(0));
    let last = stdout.rsplit("count = ").next().unwrap();
    assert!(last.starts_with('1'), "{stdout}");
    assert!(stdout.contains("marker = 9"), "{stdout}");
}

#[test]
fn emit_takes_the_events_values_and_reports_errors() {
    let (status, stdout, stderr) = repl(
        "import io { println }\nevent Move(x: Int, y: Int)\non Move(x, y) {\n    println(\"{x} {y}\")\n}\n:emit Move 1 2\n:emit Move 1\n:emit Nope\nevent Go\nlet mut n = 1\non Go {\n    n = n / 0\n}\n:emit Go\nevent Inc\non Inc {\n    n += 1\n}\n:emit Inc\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert!(stdout.contains("1 2\n"), "{stdout}");
    assert!(
        stderr.contains("`Move` carries 2 values, but 1 was given"),
        "{stderr}"
    );
    assert!(stderr.contains("unknown event `Nope`"), "{stderr}");
    assert!(
        stderr.contains("runtime error: integer division by zero"),
        "{stderr}"
    );
    // The session goes on after the error.
    let last = stdout.rsplit("n = ").next().unwrap();
    assert!(last.starts_with('2'), "{stdout}");
}
