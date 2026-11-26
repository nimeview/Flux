//! `live fn` components: each call makes an instance with its own state,
//! derived values, events, handlers, and watches. Every program runs on both
//! engines, which must agree.

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
fn both_files(files: &[(&str, &str)]) -> (Option<i32>, String, String) {
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

fn prints(src: &str, want: &str) {
    let (status, stdout, stderr) = both_files(&[("main.fl", src)]);
    assert_eq!(status, Some(0), "{src}\n{stderr}");
    assert_eq!(stdout, want, "{src}");
}

fn rejects(src: &str, needles: &[&str]) {
    let (status, _, stderr) = both_files(&[("main.fl", src)]);
    assert_eq!(status, Some(1), "{src}");
    for needle in needles {
        assert!(
            stderr.contains(needle),
            "{src}\nmissing {needle:?} in\n{stderr}"
        );
    }
}

const COUNTER: &str = r#"import io { println }

event Tick

live fn Counter(start: Int) {
    pub let mut n = start
    pub derived double = n * 2
    pub event Inc(by: Int)

    on Inc(by) {
        n += by
    }

    on Tick {
        n += 1
    }

    watch n {
        println("n={n}")
    }

    pub act fn reset() {
        n = 0
    }
}

let a = Counter(0)

let b = Counter(10)

derived total = a.double + b.double

watch total {
    println("total={total}")
}

act fn main() {
    emit a.Inc(5)
    a.reset()
    emit Tick()
    println("{a.n} {b.double}")
}
"#;

/// Module watches run before instances', and instances in the order they
/// were made; `act fn main` is one batch.
#[test]
fn instances_have_their_own_state() {
    prints(COUNTER, "total=20\nn=0\nn=10\n1 22\ntotal=24\nn=1\nn=11\n");
}

/// Lists of instances, a replaced instance, an instance inside another,
/// and instances that are dropped (they stop reacting).
#[test]
fn lists_replacement_nesting_and_drops() {
    prints(
        r#"import io { println }

event Tick
event Add(start: Int)
event Pick(i: Int)
event Clear
event Boost

live fn Counter(start: Int) {
    pub let mut n = start
    pub derived double = n * 2

    on Tick {
        n += 1
    }

    watch n {
        println("  counter n={n}")
    }
}

live fn Engine(power: Int) {
    pub let mut speed = power
    pub derived energy = speed * speed
    pub event Faster

    on Faster {
        speed += 1
    }
}

live fn Car(power: Int) {
    pub let engine = Engine(power)
    pub derived consumption = engine.energy * 2
}

let mut all: [Counter] = []
let mut current = Counter(100)
let car = Car(3)

derived sum = all.map(|c| c.double).reduce(0, |x, y| x + y)
derived shown = current.double
derived fuel = car.consumption

watch sum {
    println("sum={sum}")
}

watch shown {
    println("shown={shown}")
}

watch fuel {
    println("fuel={fuel}")
}

on Add(start) {
    all.push(Counter(start))
}

on Pick(i) {
    current = all[i]
}

on Boost {
    emit car.engine.Faster()
}

on Clear {
    all = []
}

fn main() {
    emit Add(1)
    emit Add(2)
    emit Tick
    emit Pick(1)
    emit Tick
    emit Boost
    emit Clear
    emit Tick
}
"#,
        "sum=0\nshown=200\nfuel=18\n  counter n=100\nsum=2\n  counter n=1\nsum=6\n  counter n=2\nsum=10\nshown=202\n  counter n=101\n  counter n=2\n  counter n=3\nshown=6\nsum=14\nshown=8\n  counter n=3\n  counter n=4\nfuel=32\nsum=0\nshown=10\n  counter n=5\n",
    );
}

/// A component exported by a module, an instance passed to another
/// component, and a plain function that reads members inside a derived value.
#[test]
fn components_across_modules() {
    let (status, stdout, stderr) = both_files(&[
        (
            "main.fl",
            r#"import io { println }
import counter { Counter, Label, Tick }

let a = Counter(1)
let label = Label(a, "a")
let all: [Counter] = [Counter(5), a]

derived shown = label.text

watch shown {
    println(shown)
}

fn total(xs: [Counter]) -> Int {
    xs.map(|c| c.double).reduce(0, |x, y| x + y)
}

derived sum = total(all)

watch sum {
    println("sum={sum}")
}

fn main() {
    emit Tick
    emit counter::Tick
}
"#,
        ),
        (
            "counter.fl",
            r#"pub event Tick

pub live fn Counter(start: Int) {
    pub let mut n = start
    pub derived double = n * 2

    on Tick {
        n += 1
    }
}

pub live fn Label(c: Counter, name: String) {
    pub derived text = "{name}={c.n}"
}
"#,
        ),
    ]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, "a=1\nsum=12\na=2\nsum=16\na=3\nsum=20\n");
}

/// An instance lives while something holds it; its handlers of global
/// events do not keep it alive. `==` is identity.
#[test]
fn dropped_instances_stop_reacting() {
    prints(
        r#"import io { println }

event Tick

live fn Clock(k: Int) {
    on Tick {
        println("tick {k}")
    }
}

fn make(k: Int) {
    let c = Clock(k)
}

fn main() {
    let mut i = 0
    while i < 2000 {
        make(i)
        i += 1
    }
    emit Tick
    let kept = Clock(9)
    let other = Clock(8)
    emit Tick
    println("{kept} {kept == kept} {kept == other}")
}
"#,
        "tick 9\ntick 8\nClock#2001 true false\n",
    );
}

const C: &str = "live fn C(start: Int) {
    let mut hidden = start
    pub let mut n = start
    pub event Inc(by: Int)

    on Inc(by) {
        n += by
    }

    pub act fn reset() {
        n = 0
    }
}
";

#[test]
fn outside_code_uses_only_pub_members_and_never_assigns() {
    rejects(
        &format!("{C}fn main() {{\n    let c = C(1)\n    let x = c.hidden\n}}\n"),
        &["E033", "mark it `pub` in `live fn C`"],
    );
    rejects(
        &format!("{C}act fn main() {{\n    let c = C(1)\n    c.n = 5\n}}\n"),
        &["E032", "belongs to an instance of `C`"],
    );
    rejects(
        &format!("{C}act fn main() {{\n    let c = C(1)\n    emit c.Dec(1)\n}}\n"),
        &["`Dec`, which `C` does not declare"],
    );
    rejects(
        &format!("{C}act fn main() {{\n    let c = C(1)\n    emit c.Inc(\"x\")\n}}\n"),
        &["expected `Int64`, found `String`"],
    );
    rejects(
        &format!("{C}fn main() {{\n    let c = C(1)\n    let f = c.reset\n}}\n"),
        &["`reset` read as a value"],
    );
}

#[test]
fn component_rules() {
    // Making an instance is an action: not in a derived value.
    rejects(
        &format!("{C}derived d = C(1)\nfn main() {{\n}}\n"),
        &["E008"],
    );
    rejects(
        "live fn D(mut x: Int) {\n    pub let y = x\n}\nfn main() {\n}\n",
        &["parameter `x` of a live fn cannot be `mut`"],
    );
    // A watch follows names; an instance's member needs one.
    rejects(
        &format!("{C}let a = C(1)\nwatch {{\n    let x = a.n\n}}\nfn main() {{\n}}\n"),
        &["a `watch` without a target", "`derived n = x.n`"],
    );
    // The components' graph has no cycle, however many instances there are.
    rejects(
        "live fn C(k: Int) {\n    pub derived d: Int = total + k\n}\nlet a = C(1)\nderived total: Int = a.d\nfn main() {\n}\n",
        &["E010", "C.d → total → C.d"],
    );
}

#[test]
fn components_format_stably() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), COUNTER).unwrap();
    let (status, stdout, stderr) = flux(dir.path(), &["fmt", "main.fl"]);
    assert_eq!(status, Some(0), "{stderr}");
    assert_eq!(stdout, COUNTER);
}

/// An instance made while modules initialize reads the modules' derived
/// values as they are then; a binding read before it is set is an error.
#[test]
fn instances_made_at_initialization() {
    prints(
        "import io { println }\nlet mut base = 10\nderived limit = base * 2\nlive fn C(k: Int) {\n    pub derived d = limit + k\n}\nlet a = C(1)\nderived shown = a.d\nwatch shown {\n    println(\"shown={shown}\")\n}\nfn main() {\n}\n",
        "shown=21\n",
    );
    rejects(
        "live fn C(k: Int) {\n    pub derived d = total + k\n}\nlet all = [C(1)]\nfn sum(xs: [C]) -> Int {\n    xs.map(|c| c.d).reduce(0, |x, y| x + y)\n}\nderived total: Int = sum(all)\nfn main() {\n}\n",
        &["a binding was read before its initial value was set", "at C.d"],
    );
}

/// A recursive component: an instance's value reads the same member of
/// instances made before it (its children), so no cycle can form.
#[test]
fn recursive_components() {
    prints(
        r#"import io { println }

event Grow(by: Int)

live fn Node(value: Int, children: [Node]) {
    pub let mut v = value
    pub derived total: Int = v + children.map(|c| c.total).reduce(0, |x, y| x + y)

    on Grow(by) {
        v += by
    }
}

let leaf1 = Node(1, [])
let leaf2 = Node(2, [])
let root = Node(10, [leaf1, leaf2])

derived sum = root.total

watch sum {
    println("sum={sum}")
}

fn main() {
    emit Grow(1)
}
"#,
        "sum=13\nsum=16\n",
    );
    // A long chain read from its far end, inside a batch, before the
    // batch's end recomputes it.
    prints(
        r#"import io { println }

event Grow
event Show
event Run
event Build(n: Int)

live fn Link(value: Int, next: Option[Link]) {
    pub let mut v = value
    pub derived total: Int = v + match next {
        Some(n) => n.total,
        None => 0,
    }

    on Grow {
        v += 1
    }
}

let mut head: Option[Link] = None

on Build(n) {
    let mut i = 0
    while i < n {
        head = Some(Link(1, head))
        i += 1
    }
}

on Show {
    match head {
        Some(h) => println("read in the batch: {h.total}"),
        None => println("none"),
    }
}

on Run {
    emit Grow()
    emit Show()
}

fn main() {
    emit Build(5000)
    emit Run()
}
"#,
        "read in the batch: 10000\n",
    );
}

/// A cycle through instances that only the run can see (a `let mut` that
/// comes to hold the instance itself) is a runtime error, not an old value;
/// a cycle among one component's own members is a compile error.
#[test]
fn cycles_through_instances() {
    let (status, stdout, stderr) = both_files(&[(
        "main.fl",
        r#"import io { println }

event Point

live fn Cell(k: Int) {
    pub derived d: Int = k + others.map(|c| c.d).reduce(0, |x, y| x + y)
}

let mut others: [Cell] = []
let first = Cell(1)

derived shown = first.d

watch shown {
    println("shown={shown}")
}

on Point {
    others = [first]
}

fn main() {
    emit Point
}
"#,
    )]);
    assert_eq!(status, Some(1), "{stderr}");
    assert_eq!(stdout, "shown=1\n");
    assert!(
        stderr.contains("a derived value depends on itself"),
        "{stderr}"
    );
    rejects(
        "live fn C(k: Int) {\n    pub derived a: Int = b + k\n    pub derived b: Int = a\n}\nfn main() {\n}\n",
        &["E010"],
    );
}

/// A component's `let mut` member holds a changing list of instances; one
/// removed from it is dropped and stops reacting.
#[test]
fn changing_members_hold_instances() {
    prints(
        r#"import io { println }

event Tick
event Add(text: String)
event Flip(i: Int)
event Remove(i: Int)

live fn Item(text: String) {
    pub let mut done = false
    pub event Toggle

    on Toggle {
        done = !done
    }

    on Tick {
        println("  tick {text}")
    }
}

live fn List() {
    pub let mut items: [Item] = []
    pub derived left: Int = items.filter(|i| !i.done).len()
    pub event Push(text: String)
    pub event Drop(i: Int)

    on Push(text) {
        items.push(Item(text))
    }

    on Drop(i) {
        let gone = items[i]
        items = items.filter(|x| x != gone)
    }

    watch left {
        println("left={left}")
    }
}

let list = List()

on Add(text) {
    emit list.Push(text)
}

on Flip(i) {
    let item = list.items[i]
    emit item.Toggle()
}

on Remove(i) {
    emit list.Drop(i)
}

fn main() {
    emit Add("a")
    emit Add("b")
    emit Add("c")
    emit Flip(1)
    emit Tick()
    emit Remove(0)
    emit Tick()
}
"#,
        "left=0\nleft=1\nleft=2\nleft=3\nleft=2\n  tick a\n  tick b\n  tick c\nleft=1\n  tick b\n  tick c\n",
    );
}

/// An instance may not come to hold itself, directly or through the
/// instances it holds: nothing could ever release it.
#[test]
fn an_instance_never_holds_itself() {
    let (status, stdout, stderr) = both_files(&[(
        "main.fl",
        r#"import io { println }

event Link
event Loop

live fn Node(name: String) {
    pub let mut children: [Node] = []
    pub derived size: Int = 1 + children.map(|c| c.size).reduce(0, |x, y| x + y)
    pub event Adopt(child: Node)

    on Adopt(child) {
        children.push(child)
    }
}

let root = Node("root")
let mid = Node("mid")
let leaf = Node("leaf")

derived size = root.size

watch size {
    println("size={size}")
}

on Link {
    emit mid.Adopt(leaf)
    emit root.Adopt(mid)
}

on Loop {
    emit leaf.Adopt(root)
}

fn main() {
    emit Link()
    emit Loop()
}
"#,
    )]);
    assert_eq!(status, Some(1), "{stderr}");
    assert_eq!(stdout, "size=1\nsize=3\n");
    assert!(
        stderr.contains("an instance cannot hold itself") && stderr.contains("at on Adopt"),
        "{stderr}"
    );
}

/// What an instance's watch saw does not keep instances alive.
#[test]
fn watch_memory_keeps_no_instance() {
    prints(
        r#"import io { println }

event Add(k: Int)
event Clear
event Tick

live fn W(k: Int) {
    watch all {
        println("  w{k} sees {all.len()}")
    }

    on Tick {
        println("tick {k}")
    }
}

let mut all: [W] = []

on Add(k) {
    all.push(W(k))
}

on Clear {
    all = []
}

fn main() {
    emit Add(1)
    emit Add(2)
    emit Tick()
    emit Clear()
    emit Tick()
    println("done")
}
"#,
        "  w1 sees 1\n  w1 sees 2\n  w2 sees 2\ntick 1\ntick 2\ndone\n",
    );
}
