//! Reusable reactive components: a module owns its sources, events,
//! handlers, and watches. Modules initialize dependencies first, each
//! running its watches at the end of its own initialization; another module's
//! source is read-only (E032); events are used like functions. Every program
//! runs on both engines, which must agree.

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

/// Runs `files` (the first is `main.fl`) on both engines.
fn both(files: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    for (name, src) in files {
        std::fs::write(dir.path().join(name), src).unwrap();
    }
    let vm = flux(
        dir.path(),
        &["run", "--vm", "--no-cache", "--quiet", "main.fl"],
    );
    let jit = flux(
        dir.path(),
        &["run", "--jit", "--no-cache", "--quiet", "main.fl"],
    );
    assert_eq!(vm, jit, "{}", files[0].1);
    vm
}

const COUNTER: &str = "import io { println }
pub let mut count = 0
pub derived doubled = count * 2
pub event Inc(by: Int)
on Inc(by) {
    count += by
}
pub act fn reset() {
    count = 0
}
watch count {
    println(\"counter: {count}\")
}
";

const BASE: &str = "import io { println }
pub let mut level = 1
watch level {
    println(\"base watch {level}\")
}
if true {
    println(\"base init\")
}
";

const MID: &str = "import io { println }
import base { level }
pub derived twice = level * 2
watch twice {
    println(\"mid watch {twice}\")
}
if true {
    println(\"mid init\")
}
";

#[test]
fn modules_initialize_dependencies_first_with_their_own_watches() {
    let (status, stdout, stderr) = both(&[
        (
            "main.fl",
            "import io { println }
import mid { twice }
import counter
if true {
    println(\"main init\")
}
watch twice {
    println(\"main watch {twice}\")
}
fn main() {
}
",
        ),
        ("mid.fl", MID),
        ("base.fl", BASE),
        ("counter.fl", COUNTER),
    ]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "base init\nbase watch 1\nmid init\nmid watch 2\ncounter: 0\nmain init\nmain watch 2\n"
    );
}

#[test]
fn a_module_owns_its_state_and_importers_use_its_events() {
    let (status, stdout, stderr) = both(&[
        (
            "main.fl",
            "import io { println }
import counter { doubled, Inc, reset }
import counter
derived shown = \"doubled = {doubled}\"
on counter::Inc(by) {
    println(\"main heard {by}\")
}
watch shown {
    println(shown)
}
fn main() {
    emit Inc(2)
    emit counter::Inc(3)
    reset()
}
",
        ),
        ("counter.fl", COUNTER),
    ]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "counter: 0\ndoubled = 0\nmain heard 2\ncounter: 2\ndoubled = 4\nmain heard 3\ncounter: 5\ndoubled = 10\ncounter: 0\ndoubled = 0\n"
    );
}

#[test]
fn another_modules_source_is_read_only() {
    for (name, body) in [
        (
            "handler",
            "event Go\non Go {\n    count = 1\n}\nfn main() {\n}\n",
        ),
        ("act fn", "act fn main() {\n    count += 1\n}\n"),
        ("init", "if true {\n    count = 1\n}\nfn main() {\n}\n"),
    ] {
        let main = format!("import counter {{ count }}\n{body}");
        let (status, _, stderr) = both(&[("main.fl", &main), ("counter.fl", COUNTER)]);
        assert_eq!(status, Some(1), "{name}");
        assert!(
            stderr.contains("E032") && stderr.contains("`count` belongs to another module"),
            "{name}: {stderr}"
        );
    }
}

#[test]
fn an_event_must_be_pub_and_exist() {
    let (status, _, stderr) = both(&[
        (
            "main.fl",
            "import counter\nact fn main() {\n    emit counter::Missing(1)\n}\n",
        ),
        ("counter.fl", COUNTER),
    ]);
    assert_eq!(status, Some(1));
    assert!(stderr.contains("Missing"), "{stderr}");
}

/// Review findings: an earlier module's initialization does not compute the
/// root's derived values before the root's statements ran; a module's
/// top-level `if` orders only its own statements; a watch whose value turns
/// on a dependency during initialization still runs once.
#[test]
fn initialization_groups_hold_in_edge_cases() {
    let (status, stdout, stderr) = both(&[
        (
            "main.fl",
            "import io { println }\nimport dep\nlet mut n = seed()\nderived doubled = n * 2\nfn seed() -> Int {\n    1\n}\nwatch doubled {\n    println(\"{doubled}\")\n}\nfn main() {\n}\n",
        ),
        ("dep.fl", "\n"),
    ]);
    assert_eq!((status, stdout.as_str()), (Some(0), "2\n"), "{stderr}");
    let (status, stdout, stderr) = both(&[
        ("main.fl", "import parent\nfn main() {\n}\n"),
        (
            "parent.fl",
            "import io { println }\nimport child { n }\npub let mut copy = n + 0\nif true {\n    println(\"parent {copy}\")\n}\n",
        ),
        (
            "child.fl",
            "pub let mut n = seed()\nfn seed() -> Int {\n    7\n}\n",
        ),
    ]);
    assert_eq!(
        (status, stdout.as_str()),
        (Some(0), "parent 7\n"),
        "{stderr}"
    );
    let (status, stdout, stderr) = both(&[
        (
            "main.fl",
            "import io { println }\nimport dep\nlet mut flag = false\nlet mut s = 1\nderived x = if flag { y } else { 0 }\nderived y = s + 1\nif true {\n    flag = true\n    s = 2\n}\nwatch x {\n    println(\"{x}\")\n}\nfn main() {\n}\n",
        ),
        ("dep.fl", "\n"),
    ]);
    assert_eq!((status, stdout.as_str()), (Some(0), "3\n"), "{stderr}");
}
