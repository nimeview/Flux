use std::process::{Command, Output};

fn run(source: &str, engine: &str) -> Output {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.fl"), source).unwrap();
    Command::new(env!("CARGO_BIN_EXE_flux"))
        .args(["run", engine, "--no-cache", "main.fl"])
        .current_dir(dir.path())
        .output()
        .expect("run flux")
}

fn assert_both(source: &str, expected: &str) {
    let vm = run(source, "--vm");
    let jit = run(source, "--jit");
    for (engine, output) in [("vm", &vm), ("jit", &jit)] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{engine}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{engine}"
        );
    }
    assert_eq!(vm.stdout, jit.stdout);
    assert_eq!(vm.stderr, jit.stderr);
}

#[test]
fn closures_use_real_array_and_function_types_on_both_engines() {
    assert_both(
        r#"
import io { println }
import collections { map, filter, reduce, len }
import result { Ok, Err }
import str { to_string }
struct Item { name: String, price: Int }
struct Total { value: Int }
struct Other { price: Int }
trait Price { fn price_of(i: Item) -> Int; }
impl Price for Item { fn price_of(i: Item) -> Int { i.price + 10 } }
impl Price for Other { fn price_of(i: Other) -> Int { i.price + 100 } }
fn items() -> [Item] { [Item { name: "a", price: 3 }] }
fn prices(xs: [Item]) -> [Int] { map(xs, |i| i.price) }
fn apply(f: fn(Item) -> Int, x: Item) -> Int { f(x) }
fn generic_apply[T](f: fn(T) -> Int, x: T) -> Int { f(x) }
fn inside[T](xs: [T]) -> [String] { map(xs, |i| "v={i}") }
fn via_method[T](xs: [T]) -> [Int] { map(xs, |i| i.price_of()) }
fn main() {
    let xs = [Item { name: "a", price: 3 }]
    println((map(xs, |i| i.price))[0])
    println((prices(xs))[0])
    println((map(items(), |i| i.price))[0])
    let loaded: Result[[Item], String] = Ok(xs)
    println((match loaded { Ok(items) => map(items, |i| i.price), Err(_) => [] })[0])
    println(reduce(map(xs, |i| i.price), 0, |a, b| a + b))
    let k = 4
    println((map(xs, |i| i.price + k))[0])
    println(len(filter(xs, |i| i.price > 2)))
    println(apply(|i| i.price, xs[0]))
    println(generic_apply(|i| i.price, xs[0]))
    let f: fn(Item) -> Int = |i| i.price
    println(f(xs[0]))
    println(reduce(xs, Total { value: 1 }, |a, b| Total { value: a.value + b.price }).value)
    println((map(xs, |i| "name={i.name} price={i.price}"))[0])
    println((map(xs, |i| to_string(i.price)))[0])
    println((map(xs, |i| i.price_of()))[0])
    println((inside([1, 2]))[1])
    println((inside(["a", "b"]))[1])
    println(via_method(xs)[0])
    println(via_method([Other { price: 5 }])[0])
}
"#,
        "3\n3\n3\n3\n3\n7\n1\n3\n3\n3\n4\nname=a price=3\n3\n13\nv=2\nv=b\n13\n105\n",
    );
}

#[test]
fn reactive_closure_reads_keep_their_dependencies() {
    assert_both(
        r#"
import io { println }
import collections { map }
struct Item { price: Int }
event Tick
let mut extra = 1
let xs = [Item { price: 3 }]
derived prices = map(xs, |i| i.price + extra)
on Tick { extra = 4 }
fn main() {
    println(snap(prices)[0])
    emit Tick()
    println(snap(prices)[0])
}
"#,
        "4\n7\n",
    );
}

#[test]
fn contextual_typing_preserves_annotations_and_fallback_diagnostics() {
    use flux_cli::pipeline::{compile, source_from};
    use miette::Diagnostic;
    let failure = compile(source_from(
        "<fallback>",
        "struct Item { price: Int }\nlet f = |i| i.price\n",
    ))
    .err()
    .expect("fallback error");
    let errors = &failure.diagnostics.errors;
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].code(), "E005");
    assert_eq!(
        errors[0].to_string(),
        "type mismatch: expected `field `price``, found `Int64`"
    );
    assert_eq!(errors[0].help().unwrap().to_string(), "closure parameter `i` defaulted to `Int` because it had no type annotation or call-site hint; add an explicit annotation, e.g. `|i: YourType| ...`");
    let failure = compile(source_from("<annotation>", "import collections { map }\nstruct Item { price: Int }\nfn main() { let xs = [Item { price: 3 }]\nlet ys = map(xs, |i: Int| i + 1) }\n")).err().expect("annotation mismatch");
    assert_eq!(
        failure.diagnostics.errors.len(),
        1,
        "{:?}",
        failure.diagnostics.errors
    );
    let error = &failure.diagnostics.errors[0];
    assert_eq!(error.code(), "E005");
    assert_eq!(
        error.to_string(),
        "type mismatch: expected `Item`, found `Int64`"
    );
    assert!(error.help().is_none());
}

#[test]
fn assignment_diagnostics_use_source_names_and_describe_assignments() {
    use flux_cli::pipeline::{compile, source_from};
    for (source, code, message) in [
        (
            "event T\nlet mut x = 0\nderived d = x + 1\non T { d = 3 }\n",
            "E003",
            "cannot assign to derived binding `d`",
        ),
        (
            "event T\nlet c = 0\non T { c = 1 }\n",
            "E016",
            "binding `c` is assigned; declare it with `let mut`",
        ),
    ] {
        let failure = compile(source_from("<assignment>", source))
            .err()
            .expect("assignment rejected");
        assert_eq!(
            failure.diagnostics.errors.len(),
            1,
            "{:?}",
            failure.diagnostics.errors
        );
        assert_eq!(failure.diagnostics.errors[0].code(), code);
        assert_eq!(failure.diagnostics.errors[0].to_string(), message);
    }
}

#[test]
fn generic_call_without_other_context_keeps_the_int_fallback() {
    assert_both("import io { println }\nfn only[T](f: fn(T) -> Int) -> Int { 1 }\nfn main() { println(only(|x| 1)) }\n", "1\n");
}
