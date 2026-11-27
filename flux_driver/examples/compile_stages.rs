//! Instructions each compile stage of one file takes, averaged over many
//! runs (macOS only: it reads the process's instruction counter):
//! `cargo run --release -p flux_driver --example compile_stages -- FILE [TIMES]`.

use flux_diagnostics::{DiagnosticBag, SourceId};

#[cfg(target_os = "macos")]
fn instructions() -> u64 {
    extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
        fn getpid() -> i32;
    }
    // `rusage_info_v4`: a 16-byte UUID, then 64-bit counters; the 30th is
    // `ri_instructions`.
    let mut info = [0u64; 64];
    unsafe { proc_pid_rusage(getpid(), 4, info.as_mut_ptr()) };
    info[2 + 29]
}

#[cfg(not(target_os = "macos"))]
fn instructions() -> u64 {
    0
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a .fl file");
    let times: u64 = args.next().map_or(200, |n| n.parse().expect("a count"));
    let src = std::fs::read_to_string(&path).expect("readable file");
    let names = [
        "parse",
        "HIR",
        "specialize",
        "types",
        "methods",
        "IR",
        "start",
    ];
    let mut spent = [0u64; 7];
    for _ in 0..times {
        let mut d = DiagnosticBag::new();
        let mut t = instructions();
        let mut lap = |i: usize, t: &mut u64| {
            let now = instructions();
            spent[i] += now - *t;
            *t = now;
        };
        let ast = flux_syntax::parse_program(SourceId(0), &src, &mut d).expect("parses");
        lap(0, &mut t);
        let mut hir = flux_hir::lower_program(&ast, &mut d).expect("lowers");
        lap(1, &mut t);
        flux_types::monomorphize_program(&mut hir, SourceId(0), &mut d);
        lap(2, &mut t);
        let typed = flux_types::typecheck_program(&hir, SourceId(0), &mut d).expect("checks");
        lap(3, &mut t);
        flux_types::resolve_method_calls(&mut hir, &typed);
        lap(4, &mut t);
        let ir = flux_ir::lower_to_ir(&hir, SourceId(0), &mut d).expect("IR");
        lap(5, &mut t);
        let _ = flux_runtime::Executor::with_mode(ir, flux_runtime::ExecutionMode::Vm);
        lap(6, &mut t);
    }
    for (name, total) in names.iter().zip(spent) {
        println!("{name:>10} {:8.0}k", total as f64 / times as f64 / 1000.0);
    }
}
