//! Heap lifetime under repeated JIT work: values that are no longer reachable
//! must not keep accumulating in the process-wide tagged-value pools, and
//! reclaiming them must not change results or disturb other executors.

use std::sync::{Mutex, MutexGuard};
use std::thread;

use flux_cli::pipeline::{compile_with_mode, source_from};
use flux_runtime::{ExecutionMode, Executor, RuntimeValue};
use flux_value::{heap_stats, HeapStats};

/// Heap counts are process-wide; tests in this binary must not overlap.
static HEAP_LOCK: Mutex<()> = Mutex::new(());

fn heap_lock() -> MutexGuard<'static, ()> {
    HEAP_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// Each tick allocates an array, an enum, and a new distinct string
// (`trail` grows by one character), all unreachable after the next tick.
const PROGRAM: &str = "\
enum Shape { Circle(Int), Empty }
event Tick(n: Int)
let mut total = 0
let mut trail = \"\"
fn area(s: Shape) -> Int { match s { Circle(r) => r * r, Empty => 0 } }
fn sum(xs: [Int]) -> Int {
  let mut acc = 0
  for x in xs { acc = acc + x }
  acc
}
on Tick { n ->
  total = sum([n, n + 1, n + 2]) + area(Circle(n))
  trail = trail + \"x\"
}
";

fn executor(mode: ExecutionMode) -> Executor {
    compile_with_mode(source_from("heap.fl", PROGRAM), mode)
        .unwrap_or_else(|f| panic!("{:?}", f.diagnostics.errors))
        .executor
}

fn tick(executor: &mut Executor, n: i64) {
    executor.emit("Tick", RuntimeValue::int_i64(n));
    assert!(
        executor.runtime_error().is_none(),
        "{:?}",
        executor.runtime_error()
    );
}

fn expected_total(n: i64) -> RuntimeValue {
    RuntimeValue::int_i64(3 * n + 3 + n * n)
}

#[test]
fn jit_heap_stays_bounded_over_repeated_events() {
    let _lock = heap_lock();
    let mut jit = executor(ExecutionMode::Jit);
    for n in 0..100 {
        tick(&mut jit, n);
    }
    let warm = heap_stats();
    for n in 100..2100 {
        tick(&mut jit, n);
    }
    let after = heap_stats();
    // Measured before reclamation: +2000 arrays, +2000 enums over these ticks.
    assert_eq!(after.arrays, warm.arrays, "warm={warm:?} after={after:?}");
    assert_eq!(after.enums, warm.enums, "warm={warm:?} after={after:?}");
    assert!(
        after.strings <= warm.strings,
        "a distinct string per tick must not accumulate: warm={warm:?} after={after:?}"
    );
    assert_eq!(jit.get_value("total"), Some(expected_total(2099)));
    assert_eq!(
        jit.get_value("trail"),
        Some(RuntimeValue::String("x".repeat(2100)))
    );
}

#[test]
fn reclamation_keeps_jit_results_identical_to_vm() {
    let _lock = heap_lock();
    let mut vm = executor(ExecutionMode::Vm);
    let mut jit = executor(ExecutionMode::Jit);
    for n in [0, 1, 7, -3, 40] {
        tick(&mut vm, n);
        tick(&mut jit, n);
        assert_eq!(jit.get_value("total"), vm.get_value("total"), "n={n}");
        assert_eq!(jit.get_value("trail"), vm.get_value("trail"), "n={n}");
    }
}

#[test]
fn concurrent_jit_executors_do_not_disturb_each_other() {
    let _lock = heap_lock();
    let before = heap_stats();
    thread::scope(|scope| {
        for worker in 0..4i64 {
            scope.spawn(move || {
                let mut jit = executor(ExecutionMode::Jit);
                for i in 0..500 {
                    let n = worker * 1000 + i;
                    tick(&mut jit, n);
                    assert_eq!(jit.get_value("total"), Some(expected_total(n)));
                }
                assert_eq!(
                    jit.get_value("trail"),
                    Some(RuntimeValue::String("x".repeat(500)))
                );
            });
        }
    });
    let after = heap_stats();
    assert_eq!(
        HeapStats {
            strings: 0,
            ..after
        },
        HeapStats {
            strings: 0,
            ..before
        },
        "arrays/enums left behind: before={before:?} after={after:?}"
    );
}
