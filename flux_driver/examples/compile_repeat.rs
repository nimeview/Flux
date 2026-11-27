//! Compiles one file many times in one process, for measuring compile time
//! (`/usr/bin/time -l` counts the instructions): `cargo run --release -p
//! flux_driver --example compile_repeat -- FILE [TIMES]`.

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a .fl file");
    let times: usize = args.next().map_or(200, |n| n.parse().expect("a count"));
    let src = std::fs::read_to_string(&path).expect("readable file");
    for _ in 0..times {
        let options =
            flux_driver::CompileOptions::run(flux_runtime::ExecutionMode::Vm).without_cache();
        let compiled = flux_driver::compile_with_options(
            flux_driver::source_from(path.clone(), &src),
            options,
        );
        assert!(compiled.is_ok(), "{path} does not compile");
    }
}
