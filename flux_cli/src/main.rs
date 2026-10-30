use std::fs;
use std::process;

use clap::{Parser, Subcommand};
use flux_cli::manifest::{init_project, PackageManifest};
use flux_cli::output::{print_jit_stats, print_runtime_with, report_failure, report_warnings};
use flux_cli::pipeline::{
    check_file as typecheck_file, compile_file_with_options, source_from, CompileOptions,
};
use flux_cli::repl;
use flux_diagnostics::SourceFile;
use flux_fmt::format_source;
use flux_lsp::analyze_source;
use flux_runtime::ExecutionMode;
use flux_wasm::compile_to_wasm;

#[derive(Parser)]
#[command(name = "flux", about = "Flux reactive language compiler and runner")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse, type-check, and execute a .fl file (defaults to the `flux.toml` entry point)
    Run {
        /// Path to the source file (defaults to the entry point declared in ./flux.toml)
        file: Option<String>,
        /// Enable the experimental Cranelift JIT backend
        #[arg(long, conflicts_with = "vm")]
        jit: bool,
        /// Run on the VM interpreter instead of the JIT
        #[arg(long)]
        vm: bool,
        /// Skip the project `.flux/cache` compile cache
        #[arg(long)]
        no_cache: bool,
        /// Suppress the "--- Flux runtime ---" banner
        #[arg(long)]
        quiet: bool,
        /// Also print declared events and watch/on effect handlers
        #[arg(long)]
        verbose: bool,
    },
    /// Parse and type-check without running (defaults to the `flux.toml` entry point)
    Check {
        /// Path to the source file (defaults to the entry point declared in ./flux.toml)
        file: Option<String>,
    },
    /// Interactive read-eval-print loop
    Repl {
        /// Enable the experimental Cranelift JIT backend
        #[arg(long, conflicts_with = "vm")]
        jit: bool,
        /// Run on the VM interpreter instead of the JIT
        #[arg(long)]
        vm: bool,
    },
    /// Format a .fl file (prints to stdout by default)
    Fmt {
        /// Path to the source file
        file: String,
        /// Write the formatted output back to the file instead of printing it
        #[arg(long)]
        write: bool,
        /// Exit with status 1 if the file would change
        #[arg(long)]
        check: bool,
    },
    /// Initialize a new Flux project with flux.toml
    Init {
        /// Project directory (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Package name
        #[arg(long)]
        name: Option<String>,
    },
    /// Emit LSP-style JSON diagnostics for a file (one-shot; deprecated, prefer `lsp-server`)
    Lsp {
        /// Path to the source file
        file: String,
    },
    /// Run the full stdio Language Server (diagnostics + hover + completion)
    LspServer,
    /// Experimental stub: type-checks the file, but no WASM code generation exists yet (always fails)
    Wasm {
        /// Path to the source file
        file: String,
    },
    /// Migrate v1 binding syntax to v2 (`let` / `let mut`)
    Migrate {
        /// File or directory to migrate (prints diff to stdout by default)
        path: String,
        /// Write changes in place
        #[arg(long)]
        write: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let exit = match cli.command {
        Commands::Run {
            file,
            jit,
            vm,
            no_cache,
            quiet,
            verbose,
        } => match run_file(file.as_deref(), use_jit(jit, vm), no_cache, quiet, verbose) {
            Ok(()) => 0,
            Err(()) => 1,
        },
        Commands::Check { file } => match check_file(file.as_deref()) {
            Ok(()) => 0,
            Err(()) => 1,
        },
        Commands::Repl { jit, vm } => repl::run(use_jit(jit, vm)).map_or(1, |_| 0),
        Commands::Fmt { file, write, check } => match fmt_file(&file, write, check) {
            Ok(()) => 0,
            Err(()) => 1,
        },
        Commands::Init { path, name } => match init_cmd(&path, name) {
            Ok(()) => 0,
            Err(()) => 1,
        },
        Commands::Lsp { file } => match lsp_file(&file) {
            Ok(()) => 0,
            Err(()) => 1,
        },
        Commands::LspServer => match flux_lsp::server::run() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("flux lsp-server error: {e}");
                1
            }
        },
        Commands::Wasm { file } => match wasm_file(&file) {
            Ok(()) => 0,
            Err(()) => 1,
        },
        Commands::Migrate { path, write } => match migrate_cmd(&path, write) {
            Ok(()) => 0,
            Err(()) => 1,
        },
    };
    process::exit(exit);
}

fn read_source(path: &str) -> miette::Result<SourceFile> {
    let contents =
        fs::read_to_string(path).map_err(|e| miette::miette!("failed to read {path}: {e}"))?;
    Ok(source_from(path, contents))
}

/// Resolves the file to run: the explicit `file` argument if given, otherwise
/// the entry point declared in the nearest `flux.toml` in the current
/// directory or one of its parents.
fn resolve_run_target(file: Option<&str>) -> Result<String, ()> {
    if let Some(file) = file {
        return Ok(file.to_string());
    }
    let Some(manifest_path) = find_manifest() else {
        eprintln!("no file given and no flux.toml found in this directory or any parent");
        return Err(());
    };
    let manifest = PackageManifest::load(&manifest_path).map_err(|e| {
        eprintln!("{e:?}");
    })?;
    Ok(manifest
        .entry_path(&manifest_path)
        .to_string_lossy()
        .into_owned())
}

/// The nearest `flux.toml` in the current directory or one of its parents.
fn find_manifest() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    cwd.ancestors()
        .map(|dir| dir.join("flux.toml"))
        .find(|path| path.is_file())
}

/// Engine choice: `--vm` > `--jit` > `FLUX_JIT=0|1` > `flux.toml` `[run] jit` > VM.
fn use_jit(cli_jit: bool, cli_vm: bool) -> bool {
    if cli_vm {
        return false;
    }
    if cli_jit {
        return true;
    }
    match std::env::var("FLUX_JIT").ok().as_deref() {
        Some("0") => return false,
        Some("1") => return true,
        _ => {}
    }
    find_manifest()
        .and_then(|path| PackageManifest::load(&path).ok())
        .is_some_and(|manifest| manifest.run.jit)
}

fn run_file(
    file: Option<&str>,
    jit: bool,
    no_cache: bool,
    quiet: bool,
    verbose: bool,
) -> Result<(), ()> {
    let path = resolve_run_target(file)?;
    let mode = if jit {
        ExecutionMode::Jit
    } else {
        ExecutionMode::Vm
    };
    let mut options = CompileOptions::run(mode);
    if no_cache {
        options = options.without_cache();
    }

    match compile_file_with_options(std::path::Path::new(&path), options) {
        Ok(compiled) => {
            let _ = report_warnings(&compiled.source, &compiled.sources, &compiled.warnings);
            if !quiet {
                println!("--- Flux runtime ---");
            }
            print_runtime_with(&compiled.executor, verbose);
            if verbose {
                if let Some(stats) = compiled.executor.jit_stats() {
                    print_jit_stats(&stats);
                }
            }
            Ok(())
        }
        Err(failed) => report_failure(failed.source(), &failed.sources, &failed.diagnostics),
    }
}

fn check_file(file: Option<&str>) -> Result<(), ()> {
    let path = resolve_run_target(file)?;
    match typecheck_file(std::path::Path::new(&path)) {
        Ok(compiled) => {
            let _ = report_warnings(&compiled.source, &compiled.sources, &compiled.warnings);
            println!("ok");
            Ok(())
        }
        Err(failed) => report_failure(failed.source(), &failed.sources, &failed.diagnostics),
    }
}

fn fmt_file(path: &str, write: bool, check: bool) -> Result<(), ()> {
    let contents = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("failed to read {path}: {e}");
    })?;
    let formatted = format_source(&contents).map_err(|diagnostics| {
        for diag in &diagnostics.errors {
            eprintln!("{diag}");
        }
    })?;
    if check {
        if formatted != contents {
            eprintln!("{path}: would reformat");
            return Err(());
        }
        return Ok(());
    }
    if write {
        std::fs::write(path, &formatted).map_err(|e| {
            eprintln!("failed to write {path}: {e}");
        })?;
    } else {
        print!("{formatted}");
    }
    Ok(())
}

fn init_cmd(path: &str, name: Option<String>) -> Result<(), ()> {
    let dir = std::path::Path::new(path);
    let pkg_name = name.unwrap_or_else(|| {
        dir.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("my_app")
            .to_string()
    });
    init_project(dir, &pkg_name).map_err(|e| {
        eprintln!("{e:?}");
    })?;
    println!("Created Flux project `{pkg_name}` in {}", dir.display());
    Ok(())
}

fn lsp_file(path: &str) -> Result<(), ()> {
    let contents = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("failed to read {path}: {e}");
    })?;
    let diags = analyze_source(path, &contents);
    println!(
        "{}",
        serde_json::to_string_pretty(&diags).unwrap_or_else(|_| "[]".into())
    );
    Ok(())
}

fn wasm_file(path: &str) -> Result<(), ()> {
    let source = read_source(path).map_err(|e| {
        eprintln!("{e:?}");
    })?;
    // Type-check and lower only: the program must not run (or touch the
    // cache) just to discover that no WASM backend exists yet.
    let checked = match typecheck_file(std::path::Path::new(path)) {
        Ok(checked) => checked,
        Err(failed) => {
            return report_failure(failed.source(), &failed.sources, &failed.diagnostics)
        }
    };
    let mut diagnostics = flux_diagnostics::DiagnosticBag::new();
    match flux_ir::lower_to_ir(&checked.hir, source.id, &mut diagnostics) {
        Some(program) => {
            let (status, bytes) = compile_to_wasm(&program);
            // Never report success for output that omits the program.
            eprintln!(
                "error: the WASM backend is an experimental stub ({status:?}): it produces only \
                 an empty {}-byte module and compiles none of `{path}`; nothing was written",
                bytes.len()
            );
            Err(())
        }
        None => report_failure(&source, std::slice::from_ref(&source), &diagnostics),
    }
}

fn migrate_cmd(path: &str, write: bool) -> Result<(), ()> {
    use flux_cli::migrate::{migrate_file, migrate_source, migrate_tree};

    let p = std::path::Path::new(path);
    if p.is_dir() {
        let results = migrate_tree(p, write).map_err(|e| {
            eprintln!("{e:?}");
        })?;
        for (file, changed) in results {
            if changed {
                if write {
                    println!("migrated {}", file.display());
                } else {
                    let input = fs::read_to_string(&file).map_err(|e| {
                        eprintln!("read {}: {e}", file.display());
                    })?;
                    let output = migrate_source(&input);
                    println!("--- {}\n+++ migrated\n{output}", file.display());
                }
            }
        }
        return Ok(());
    }

    let (output, changed) = migrate_file(p, write).map_err(|e| {
        eprintln!("{e:?}");
    })?;
    if write {
        if changed {
            println!("migrated {}", p.display());
        } else {
            println!("no changes needed for {}", p.display());
        }
    } else if changed {
        print!("{output}");
    } else {
        let input = fs::read_to_string(p).map_err(|e| {
            eprintln!("read {}: {e}", p.display());
        })?;
        print!("{input}");
    }
    Ok(())
}
