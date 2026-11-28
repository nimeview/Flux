//! The host contract: a host gives a program its `extern fn`s, sends it
//! events from other threads, and runs its timers.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use flux_driver::{compile_file_with_host, CompileOptions};
use flux_runtime::{ExecutionMode, Host, RuntimeValue};

/// Writes `src` to a file of its own and compiles it for `host`.
fn compile(name: &str, src: &str, host: Host) -> Result<flux_driver::Compiled, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("host-test-{name}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let main = root.join("main.fl");
    fs::write(&main, src).unwrap();
    let options = CompileOptions::run(ExecutionMode::Vm).without_cache();
    compile_file_with_host(&main, options, host)
        .map_err(|failed| format!("{:?}", failed.diagnostics))
}

#[test]
fn extern_functions_come_from_the_host() {
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = Arc::clone(&log);
    let host = Host::new()
        .function("double", |args| match args {
            [RuntimeValue::Int { .. }] => Ok(RuntimeValue::int_i64(args[0].as_int().unwrap() * 2)),
            _ => Err("expected an Int".into()),
        })
        .function("log", move |args| {
            seen.lock().unwrap().push(args[0].display_string());
            Ok(RuntimeValue::Unit)
        });
    let src = "extern fn double(n: Int) -> Int\nextern fn log(text: String)\n\nfn main() -> Int {\n    log(\"start\")\n    double(21)\n}\n";
    let mut compiled = compile("extern", src, host).expect("compiles");
    let result = compiled.executor.call("main", &[]).expect("runs");
    assert_eq!(result.as_int(), Some(42));
    assert_eq!(*log.lock().unwrap(), vec!["\"start\"".to_string()]);
}

#[test]
fn a_missing_host_function_stops_the_start() {
    let src = "extern fn ask(q: String) -> Int\n\nfn main() {\n}\n";
    let err = match compile("missing", src, Host::new()) {
        Ok(_) => panic!("must not start"),
        Err(err) => err,
    };
    assert!(err.contains("this host does not provide `ask`"), "{err}");
}

#[test]
fn a_host_function_can_fail() {
    let host = Host::new().function("ask", |_| Err("no answer".into()));
    let src = "extern fn ask(q: String) -> Int\n\nfn main() -> Int {\n    ask(\"?\")\n}\n";
    let mut compiled = compile("fails", src, host).expect("compiles");
    let err = compiled.executor.call("main", &[]).expect_err("fails");
    let text = format!("{err:#}");
    assert!(text.contains("a host function failed"), "{text}");
    assert!(text.contains("`ask`: no answer"), "{text}");
}

#[test]
fn events_sent_from_another_thread_run_in_order() {
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = Arc::clone(&log);
    let host = Host::new().function("record", move |args| {
        seen.lock().unwrap().push(args[0].display_string());
        Ok(RuntimeValue::Unit)
    });
    let src = "import time { stop }\n\nextern fn record(text: String)\n\nevent Say(text: String)\nevent Quit\n\non Say(text) {\n    record(text)\n}\n\non Quit {\n    stop()\n}\n";
    let mut compiled = compile("events", src, host).expect("compiles");
    let sender = compiled.executor.event_sender();
    let thread = std::thread::spawn(move || {
        for word in ["a", "b", "c"] {
            sender.send("Say", vec![RuntimeValue::String(word.into())]);
        }
        sender.send("Quit", vec![]);
    });
    compiled.executor.run_event_loop().expect("runs");
    thread.join().unwrap();
    assert_eq!(*log.lock().unwrap(), vec!["\"a\"", "\"b\"", "\"c\""]);
    assert!(compiled.executor.stopped());
}

/// A sender that posts and goes while the loop is busy with its earlier
/// events: the later ones still run (0.5: the loop saw no sender left and
/// ended with them queued). The handler is slow on purpose, so the sender
/// finishes while it runs.
#[test]
fn events_from_a_sender_that_just_left_still_run() {
    for round in 0..10 {
        let count: Arc<Mutex<usize>> = Arc::default();
        let seen = Arc::clone(&count);
        let host = Host::new().function("hit", move |_| {
            std::thread::sleep(std::time::Duration::from_millis(5));
            *seen.lock().unwrap() += 1;
            Ok(RuntimeValue::Unit)
        });
        let src = "extern fn hit()\n\nevent Hit\n\non Hit {\n    hit()\n}\n";
        let mut compiled = compile("late-sender", src, host).expect("compiles");
        let sender = compiled.executor.event_sender();
        let thread = std::thread::spawn(move || {
            sender.send("Hit", vec![]);
            std::thread::sleep(std::time::Duration::from_millis(1));
            sender.send("Hit", vec![]);
            sender.send("Hit", vec![]);
        });
        compiled.executor.run_event_loop().expect("runs");
        thread.join().unwrap();
        assert_eq!(*count.lock().unwrap(), 3, "round {round}");
    }
}
