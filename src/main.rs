use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: fluxc <input.fx> <output.fxbc>");
        std::process::exit(1);
    }

    let input = &args[1];
    let output = &args[2];

    if !input.ends_with(".fx") {
        eprintln!("input file must use .fx extension");
        std::process::exit(1);
    }

    let bytecode = compiler::compile_file_to_binary_bytecode(Path::new(input)).unwrap_or_else(|e| {
        eprintln!("compile error: {e}");
        std::process::exit(1);
    });

    if let Some(parent) = Path::new(output).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("failed to create output directory: {e}");
                std::process::exit(1);
            }
        }
    }

    fs::write(output, bytecode).unwrap_or_else(|e| {
        eprintln!("failed to write {output}: {e}");
        std::process::exit(1);
    });
}
