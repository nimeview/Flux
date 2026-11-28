//! Function kinds and how state changes: `fn`, `track fn`, `act fn`, and the
//! older `rfn`. Every program runs on both engines, which must agree.

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
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn track_fn_makes_its_callers_depend_on_what_it_reads() {
    prints(
        "import io { println }\nevent Up\nlet mut factor = 2\ntrack fn scale(x: Int) -> Int {\n    x * factor\n}\nlet total = scale(3)\non Up {\n    factor = 3\n}\nwatch total {\n    println(\"{total}\")\n}\nfn main() {\n    emit Up\n}\n",
        "6\n9\n",
    );
}

#[test]
fn act_fn_reads_current_values_and_returns_a_result() {
    prints(
        "import io { println }\nlet mut balance = 10\nact fn withdraw(n: Int) -> Result[Int, String] {\n    if n > balance {\n        return Err(\"not enough\")\n    }\n    balance = balance - n\n    Ok(balance)\n}\nfn main() {\n    println(\"{withdraw(3)} {withdraw(50)}\")\n}\n",
        "Ok(7) Err(\"not enough\")\n",
    );
}

#[test]
fn rfn_still_works_and_names_its_replacement() {
    let (status, stdout, stderr) = both(
        "import io { println }\nlet mut factor = 2\nrfn scale(x: Int) -> Int {\n    x * factor\n}\nlet total = scale(3)\nfn main() {\n    println(\"{snap(total)}\")\n}\n",
    );
    assert_eq!(status, Some(0));
    assert_eq!(stdout, "6\n");
    assert!(stderr.contains("flux::E026"), "{stderr}");
    assert!(
        stderr.contains("`rfn` is now written `track fn`"),
        "{stderr}"
    );
}

#[test]
fn migrate_rewrites_rfn_and_the_program_behaves_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let before = "import io { println }\n\nlet mut factor = 2\n\n// rfn in a comment stays\nrfn scale(x: Int) -> Int {\n    x * factor\n}\n\npub rfn twice(x: Int) -> Int {\n    scale(x) * 2\n}\n\nlet total = twice(3)\n\nfn main() {\n    println(\"{snap(total)} rfn\")\n}\n";
    std::fs::write(dir.path().join("main.fl"), before).unwrap();
    let run = ["run", "--vm", "--no-cache", "--quiet", "main.fl"];
    let (_, out_before, _) = flux(dir.path(), &run);

    let (status, _, stderr) = flux(dir.path(), &["migrate", "--write", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    let after = std::fs::read_to_string(dir.path().join("main.fl")).unwrap();
    assert_eq!(
        after,
        before
            .replace("\nrfn scale", "\ntrack fn scale")
            .replace("pub rfn", "pub track fn")
    );

    let (status, out_after, stderr) = flux(dir.path(), &run);
    assert_eq!(status, Some(0));
    assert_eq!(out_after, out_before);
    assert!(!stderr.contains("E026"), "{stderr}");
    // Running it again changes nothing.
    flux(dir.path(), &["migrate", "--write", "main.fl"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("main.fl")).unwrap(),
        after
    );
}

/// The warning codes in `stderr`, in order.
fn codes(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter_map(|l| l.trim().strip_prefix("flux::"))
        .collect()
}

#[test]
fn writes_outside_handlers_and_act_fns_are_errors() {
    let (status, stdout, stderr) = both(
        "import io { println }\nlet mut n = 0\nlet mut seen = 0\nlet twice = n * 2\nfn bump() {\n    n = snap(n) + 1\n    n = snap(n) + 1\n    seen = 1\n}\ntrack fn peek() -> Int {\n    seen = 2\n    n\n}\nwatch twice {\n    seen = seen + 10\n}\nfn main() {\n    bump()\n    println(\"{snap(n)} {snap(seen)}\")\n}\n",
    );
    // A `fn` or `track fn` that changes a source is an error (E027), once
    // per source and function; a watch that does warns (E028).
    assert_eq!(status, Some(1), "{stderr}");
    assert!(stdout.is_empty(), "nothing runs: {stdout}");
    assert_eq!(codes(&stderr), ["E028", "E027", "E027", "E027"], "{stderr}");
    assert!(
        stderr.contains("`bump` changes `n`, but only `on` handlers and `act fn` change state"),
        "{stderr}"
    );
    assert!(stderr.contains("write `act fn bump`"), "{stderr}");
    assert!(stderr.contains("this watch changes `seen`"), "{stderr}");
}

#[test]
fn handlers_act_fns_initialization_and_locals_do_not_warn() {
    prints(
        "import io { println }\nimport collections { map }\nevent Go\nlet mut n = 0\nlet mut log = 0\nif true {\n    n = 1\n}\nact fn add(k: Int) -> Int {\n    n = n + k\n    n\n}\non Go {\n    let xs = map([1, 2], |x| x + n)\n    log = xs[1]\n}\nfn helper() -> Int {\n    let mut local = 1\n    local = local + 1\n    local\n}\nfn main() {\n    let r = add(helper())\n    emit Go\n    println(\"{r} {snap(log)}\")\n}\n",
        "3 5\n",
    );
}

#[test]
fn migrate_makes_a_writing_fn_an_act_fn() {
    let dir = tempfile::tempdir().unwrap();
    let before = "import io { println }\n\nlet mut total = 0\n\npub fn add(k: Int) {\n    total = snap(total) + k\n}\n\nfn main() {\n    add(2)\n    add(3)\n    println(\"{snap(total)}\")\n}\n";
    std::fs::write(dir.path().join("main.fl"), before).unwrap();
    let run = ["run", "--vm", "--no-cache", "--quiet", "main.fl"];
    let (status, _, stderr) = flux(dir.path(), &run);
    assert_eq!(status, Some(1), "{stderr}");
    assert_eq!(codes(&stderr), ["E027"], "{stderr}");

    let (status, _, stderr) = flux(dir.path(), &["migrate", "--write", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    let after = std::fs::read_to_string(dir.path().join("main.fl")).unwrap();
    assert_eq!(after, before.replace("pub fn add", "pub act fn add"));

    let (status, out_after, stderr) = flux(dir.path(), &run);
    assert_eq!(status, Some(0));
    assert_eq!(out_after, "5\n");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn an_act_fn_call_is_one_batch() {
    prints(
        "import io { println }\nlet mut a = 0\nlet mut b = 0\nlet sum = a + b\nwatch sum {\n    println(\"sum={sum}\")\n}\nact fn set_both(x: Int) {\n    a = x\n    b = x\n    println(\"set\")\n}\nfn main() {\n    set_both(2)\n    set_both(3)\n}\n",
        "sum=0\nset\nsum=4\nset\nsum=6\n",
    );
}

#[test]
fn every_handler_and_nested_emit_joins_the_outer_batch() {
    prints(
        "import io { println }\nevent T\nevent U\nlet mut x = 0\nlet mut y = 0\nwatch x {\n    println(\"x={x}\")\n}\nwatch y {\n    println(\"y={y}\")\n}\nact fn bump_y() {\n    y = y + 1\n}\non T {\n    x = 1\n    emit U\n    bump_y()\n}\non T {\n    x = x + 10\n}\non U {\n    y = 5\n}\nfn main() {\n    emit T\n    println(\"end\")\n}\n",
        "x=0\ny=0\nx=11\ny=6\nend\n",
    );
}

#[test]
fn a_watch_emit_settles_in_the_same_batch_and_each_watch_runs_once() {
    prints(
        "import io { println }\nevent T\nevent Count\nlet mut x = 0\nlet mut n = 0\nwatch x {\n    emit Count\n}\nwatch n {\n    println(\"n={n}\")\n}\non Count {\n    n = n + 1\n}\non T {\n    x = x + 1\n}\nfn main() {\n    emit T\n    emit T\n}\n",
        "n=1\nn=2\nn=3\n",
    );
}

#[test]
fn a_failed_batch_keeps_earlier_writes_and_runs_nothing_after() {
    let (status, stdout, stderr) = both(
        "import io { println }\nevent T\nlet mut a = 1\nlet mut log = 0\nlet d = 10 / a\nwatch d {\n    println(\"d={d}\")\n}\nact fn fail() {\n    log = 7\n    a = 0\n    println(\"d is {d}\")\n}\nfn main() {\n    fail()\n    println(\"never\")\n}\n",
    );
    assert_eq!(status, Some(1));
    assert_eq!(stdout, "d=10\n");
    assert!(
        stderr.contains("integer division by zero in `d`"),
        "{stderr}"
    );
}

#[test]
fn a_watch_runs_only_when_its_value_differs_from_what_it_saw() {
    prints(
        "import io { println }\nevent Same\nevent Back\nevent Up\nlet mut x = 0\nlet mut a = 1\nlet big = x > 5\non Same {\n    x = x\n}\non Back {\n    x = 9\n    x = 0\n}\non Up {\n    x = x + 1\n}\nwatch x(prev) {\n    println(\"x {prev} -> {x}\")\n}\nwatch big {\n    println(\"big={big}\")\n}\nwatch {\n    println(\"a={a} big={big}\")\n}\nfn main() {\n    emit Same\n    emit Back\n    emit Up\n    emit Up\n}\n",
        "x 0 -> 0\nbig=false\na=1 big=false\nx 0 -> 1\nx 1 -> 2\n",
    );
}

#[test]
fn a_watch_compares_right_before_it_runs() {
    // The first watch puts `y` back before the second one's turn comes.
    let (status, stdout, stderr) = both(
        "import io { println }\nevent T\nlet mut x = 0\nlet mut y = 0\non T {\n    x = 1\n    y = 5\n}\nwatch x {\n    y = 0\n}\nwatch y {\n    println(\"y={y}\")\n}\nfn main() {\n    emit T\n}\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "y=0\n");
    assert_eq!(codes(&stderr), ["E028"], "{stderr}");
}

#[test]
fn prev_has_the_type_of_what_the_watch_follows() {
    let (status, _, stderr) = both(
        "import io { println }\nlet mut name = \"a\"\nwatch name(old) {\n    let n: Int = old\n    println(\"{n}\")\n}\nfn main() {\n}\n",
    );
    assert_eq!(status, Some(1));
    assert!(stderr.contains("type mismatch"), "{stderr}");
    let (status, _, stderr) = both("let mut x = 0\nwatch (prev) {\n}\nfn main() {\n}\n");
    assert_eq!(status, Some(1));
    assert!(stderr.contains("parse error"), "{stderr}");
}

#[test]
fn watch_prev_survives_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let src = "let mut x = 0\n\nwatch x( old ) {\n}\n\nfn main() {\n}\n";
    std::fs::write(dir.path().join("main.fl"), src).unwrap();
    let (status, stdout, stderr) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(stdout.contains("watch x(old) {\n"), "{stdout}");
}

#[test]
fn an_event_carries_several_values_in_order() {
    prints(
        "import io { println }\nevent Move(dx: Int, dy: Int, label: String)\nevent Ping\nlet mut x = 0\nlet mut y = 0\non Move(dx, dy, label) {\n    x = x + dx\n    y = y + dy\n    println(\"{label}: {x},{y}\")\n}\non Move {\n    println(\"moved\")\n}\non Ping() {\n    println(\"ping\")\n}\nfn main() {\n    emit Move(1, 2, \"a\")\n    emit Move(-3, 5, \"b\")\n    emit Ping()\n}\n",
        "a: 1,2\nmoved\nb: -2,7\nmoved\nping\n",
    );
}

#[test]
fn the_arrow_form_still_works_and_migrate_rewrites_it() {
    let dir = tempfile::tempdir().unwrap();
    let before = "import io { println }\n\nevent Tick(n: Int)\n\nlet mut total = 0\n\non Tick { n ->\n    total = total + n\n    println(\"{total}\")\n}\n\nfn main() {\n    emit Tick(2)\n    emit Tick(3)\n}\n";
    std::fs::write(dir.path().join("main.fl"), before).unwrap();
    let run = ["run", "--vm", "--no-cache", "--quiet", "main.fl"];
    let (status, out_before, stderr) = flux(dir.path(), &run);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(out_before, "2\n5\n");
    assert!(
        stderr.contains("`on Tick { n -> }` is now written `on Tick(n) { }`"),
        "{stderr}"
    );
    let (status, _, stderr) = flux(dir.path(), &["migrate", "--write", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    let after = std::fs::read_to_string(dir.path().join("main.fl")).unwrap();
    assert_eq!(after, before.replace("on Tick { n ->", "on Tick(n) {"));
    let (status, out_after, stderr) = flux(dir.path(), &run);
    assert_eq!(status, Some(0));
    assert_eq!(out_after, out_before);
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn watch_cycles_warn_once_per_cycle() {
    let (status, _, stderr) = both(
        "event Bump\nlet mut a = 0\nlet mut b = 0\nlet mut c = 0\nlet twice_b = b * 2\nact fn set_a(v: Int) {\n    a = v\n}\non Bump {\n    c = c + 1\n}\nwatch a {\n    b = a + 1\n}\nwatch twice_b {\n    set_a(twice_b)\n}\nwatch c {\n    c = c + 1\n}\nwatch {\n    emit Bump()\n}\nfn main() {\n}\n",
    );
    assert_eq!(status, Some(0), "{stderr}");
    let cycles: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("flux::E029"))
        .collect();
    assert_eq!(cycles.len(), 2, "{stderr}");
    assert!(
        stderr.contains("this watch and 1 other wake each other in a cycle through `a`, `b`"),
        "{stderr}"
    );
    assert!(
        stderr.contains("this watch wakes itself through `c`"),
        "{stderr}"
    );
}

#[test]
fn watches_that_only_feed_forward_do_not_warn() {
    let (_, _, stderr) = both(
        "event Go\nlet mut a = 0\nlet mut b = 0\nlet mut seen = 0\non Go {\n    a = a + 1\n}\nwatch a {\n    emit Go2()\n}\nevent Go2\non Go2 {\n    b = b + 1\n}\nwatch b {\n    seen = 1\n}\nfn main() {\n    emit Go()\n}\n",
    );
    assert!(!stderr.contains("E029"), "{stderr}");
}

#[test]
fn a_handler_reads_current_values_through_act_fns_and_snap() {
    prints(
        "import io { println }\nevent Go\nlet mut x = 1\nlet d = x * 2\nlet mut y = 0\nlet mut z = 0\nact fn read() -> Int {\n    d\n}\nfn peek() -> Int {\n    snap(d)\n}\non Go {\n    x = 2\n    y = read()\n    z = peek()\n}\nfn main() {\n    emit Go\n    println(\"{snap(y)} {snap(z)}\")\n}\n",
        "4 4\n",
    );
}

#[test]
fn watch_cycles_follow_methods_and_function_values_but_not_act_fn_reads() {
    let warns = |src: &str| {
        let (_, _, stderr) = both(src);
        stderr.contains("this watch wakes itself through `n`")
    };
    assert!(warns(
        "let mut n = 0\nact fn add(k: Int) {\n    n = n + k\n}\ntrait Bump {\n    fn bump(self: Int) -> Unit;\n}\nimpl Bump for Int {\n    fn bump(self: Int) {\n        add(self)\n    }\n}\nwatch n {\n    (1).bump()\n}\nfn main() {\n}\n"
    ));
    assert!(warns(
        "let mut n = 0\nact fn bump() {\n    n = n + 1\n}\nlet callback = bump\nwatch n {\n    callback()\n}\nfn main() {\n}\n"
    ));
    // An `act fn` reads without tracking, so this watch follows nothing.
    assert!(!warns(
        "let mut n = 0\nact fn bump() {\n    n = n + 1\n}\nwatch {\n    bump()\n}\nfn main() {\n}\n"
    ));
}

#[test]
fn a_method_call_on_a_source_is_e027_only_if_it_changes_the_source() {
    // A type's own `push` that only reads is not a write, in any function,
    // and when it is the last expression it is the function's value.
    prints(
        "import io { println }\nstruct S {\n    n: Int\n}\nimpl S {\n    fn push(self, x: Int) -> Int {\n        self.n + x\n    }\n}\nlet mut s = S { n: 1 }\ntrack fn f() -> Int {\n    s.push(1)\n}\nfn g(t: S) -> Int {\n    t.push(10)\n}\nfn main() {\n    println(\"{snap(f())} {g(S { n: 1 })}\")\n}\n",
        "2 11\n",
    );
    // A `mut self` method and the library's `push` change the source.
    for src in [
        "struct S {\n    n: Int\n}\nimpl S {\n    fn bump(mut self) {\n        self.n += 1\n    }\n}\nlet mut s = S { n: 1 }\nfn f() {\n    s.bump()\n}\nfn main() {}\n",
        "let mut xs = [1]\nfn f() {\n    xs.push(2)\n}\nfn main() {}\n",
    ] {
        let (status, _, stderr) = both(src);
        assert_eq!(status, Some(1), "{src}");
        assert!(stderr.contains("`f` changes"), "{stderr}");
    }
}

#[test]
fn migrate_fixes_every_writing_fn_in_one_go() {
    // A failing function body used to leave its parameters in scope, hiding
    // the next functions' writes until another round.
    let mut src: String = (0..9).map(|i| format!("let mut x{i} = 0\n")).collect();
    for i in 0..9 {
        let params: Vec<String> = (i + 1..9).map(|j| format!("x{j}: Int")).collect();
        src += &format!(
            "fn f{i}({}) {{\n    x{i} = x{i} + 1\n}}\n",
            params.join(", ")
        );
    }
    src += "fn main() {}\n";
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), &src).unwrap();
    let (_, _, stderr) = flux(dir.path(), &["check", "main.fl"]);
    assert_eq!(stderr.matches("flux::E027").count(), 9, "{stderr}");
    flux(dir.path(), &["migrate", "--write", "main.fl"]);
    let after = std::fs::read_to_string(dir.path().join("main.fl")).unwrap();
    assert_eq!(after.matches("act fn f").count(), 9, "{after}");
    let (status, _, stderr) = flux(dir.path(), &["check", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
}

/// The E029 check is one pass over the graph: a watch at the end of 2,000
/// chained derived values checks quickly (0.5: the old fixed point took a
/// round per link, 9.5 s for 1,000 on a release build).
#[test]
fn a_long_derived_chain_with_a_watch_checks_quickly() {
    let mut src = String::from("let mut s = 0\n\nderived d0 = s + 1\n");
    for i in 1..2000 {
        src.push_str(&format!("derived d{i} = d{} + 1\n", i - 1));
    }
    src.push_str("\nwatch d1999 {\n}\n\nfn main() {\n}\n");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), &src).unwrap();
    let started = std::time::Instant::now();
    let (status, _, stderr) = flux(dir.path(), &["check", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}
