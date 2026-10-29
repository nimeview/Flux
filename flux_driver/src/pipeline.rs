//! Unified compile pipeline: parse → resolve → HIR → types → IR → runtime.
//!
//! Used by the CLI, LSP, REPL and integration tests so every entry point runs
//! the same phases in the same order.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use flux_diagnostics::{
    span_to_miette, DiagnosticBag, FluxError, FluxWarning, SourceFile, SourceId, Span,
};
use flux_hir::{lower_linked_program, lower_program, HirProgram};
use flux_ir::{Codegen, CompiledProgram, IrCodegen};
use flux_resolve::{find_project_root, resolve_file, resolve_file_with_root, ResolvedProgram};
use flux_runtime::{ExecutionMode, Executor};
use flux_syntax::{parse_program, Program};
use flux_types::{typecheck_program, TypedProgram};

use crate::cache::{get_analysis_cache, put_analysis_cache, CachedAnalysis, CompileCache};
use crate::options::{CompileOptions, CompilePhase};

static COMPILE_CACHE: OnceLock<Mutex<HashMap<PathBuf, CompileCache>>> = OnceLock::new();

fn compile_caches() -> &'static Mutex<HashMap<PathBuf, CompileCache>> {
    COMPILE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn resolve_project_root(explicit: Option<&Path>, source_file: Option<&Path>) -> PathBuf {
    if let Some(root) = explicit {
        return root.to_path_buf();
    }
    source_file
        .map(find_project_root)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|cwd| find_project_root(&cwd))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

fn with_project_cache<R>(project_root: &Path, f: impl FnOnce(&mut CompileCache) -> R) -> R {
    let key = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let mut caches = compile_caches()
        .lock()
        .expect("compile cache mutex poisoned");
    let cache = caches
        .entry(key)
        .or_insert_with(|| CompileCache::for_project(project_root));
    f(cache)
}

/// Type-check result — no IR or executor.
pub struct Checked {
    pub source: SourceFile,
    pub hir: HirProgram,
    pub typed: TypedProgram,
    pub types: HashMap<String, String>,
    pub warnings: Vec<FluxWarning>,
}

/// Successful compilation result.
pub struct Compiled {
    pub source: SourceFile,
    pub hir: HirProgram,
    pub typed: TypedProgram,
    pub executor: Executor,
    /// Display types keyed by binding / event / fn name.
    pub types: HashMap<String, String>,
    /// Non-fatal warnings collected during compilation (e.g. E015 unused bindings).
    pub warnings: Vec<FluxWarning>,
}

/// Result of an analysis-only compile (LSP / diagnostics).
pub struct Analyzed {
    pub diagnostics: DiagnosticBag,
    pub hir: Option<HirProgram>,
    pub typed: Option<TypedProgram>,
    /// Maps each loaded module's [`SourceId`] to its on-disk path.
    pub source_paths: HashMap<SourceId, String>,
}

/// Compilation failed — diagnostics are ready to render.
#[derive(Debug)]
pub struct CompileFailed {
    pub source: SourceFile,
    pub diagnostics: DiagnosticBag,
}

struct Artifacts {
    source: SourceFile,
    hir: HirProgram,
    typed: TypedProgram,
    types: HashMap<String, String>,
    ir: Option<CompiledProgram>,
}

/// Compile Flux source from memory with default VM execution.
pub fn compile(source: SourceFile) -> Result<Compiled, CompileFailed> {
    compile_with_options(source, CompileOptions::default())
}

/// Compile with an explicit execution engine (`Vm` or `Jit`).
pub fn compile_with_mode(
    source: SourceFile,
    mode: ExecutionMode,
) -> Result<Compiled, CompileFailed> {
    compile_with_options(source, CompileOptions::run(mode))
}

/// Compile with full control over pipeline phases and caching.
pub fn compile_with_options(
    source: SourceFile,
    options: CompileOptions,
) -> Result<Compiled, CompileFailed> {
    let mut diagnostics = DiagnosticBag::new();

    let Some(ast) = parse_program(source.id, &source.contents, &mut diagnostics) else {
        return Err(CompileFailed {
            source,
            diagnostics,
        });
    };

    let artifacts = lower_to_ir(source, ast, &options, None, None, &mut diagnostics)?;
    if diagnostics.has_errors() {
        return Err(CompileFailed {
            source: artifacts.source,
            diagnostics,
        });
    }

    let warnings = std::mem::take(&mut diagnostics.warnings);
    let Some(ir) = artifacts.ir else {
        return Err(CompileFailed {
            source: artifacts.source,
            diagnostics,
        });
    };
    let executor = Executor::with_mode(ir, options.mode).map_err(|msg| {
        diagnostics.push(FluxError::Parse {
            message: format!("VM validation failed: {msg}"),
            span: span_to_miette(Span {
                source: artifacts.source.id,
                start: 0,
                end: 0,
            }),
            source_id: artifacts.source.id,
        });
        CompileFailed {
            source: artifacts.source.clone(),
            diagnostics,
        }
    })?;
    Ok(Compiled {
        source: artifacts.source,
        hir: artifacts.hir,
        typed: artifacts.typed,
        executor,
        types: artifacts.types,
        warnings,
    })
}

/// Type-check a `.fl` file from disk (parse → resolve → HIR → types; no IR or VM).
pub fn check_file(path: &Path) -> Result<Checked, CompileFailed> {
    let mut diagnostics = DiagnosticBag::new();
    let Some(resolved) = resolve_file(path, &mut diagnostics) else {
        let source = SourceFile {
            id: SourceId(0),
            path: path.display().to_string(),
            contents: String::new(),
        };
        return Err(CompileFailed {
            source,
            diagnostics,
        });
    };
    let root = resolved.root.clone();
    let project_root = find_project_root(path);
    let cache_key = Some(CompileCache::cache_key_resolved(&resolved));
    let artifacts = lower_to_ir_resolved(
        root,
        &resolved,
        &CompileOptions::check(),
        cache_key,
        Some(project_root.as_path()),
        &mut diagnostics,
    )?;
    if diagnostics.has_errors() {
        return Err(CompileFailed {
            source: artifacts.source,
            diagnostics,
        });
    }
    Ok(Checked {
        source: artifacts.source,
        hir: artifacts.hir,
        typed: artifacts.typed,
        types: artifacts.types,
        warnings: std::mem::take(&mut diagnostics.warnings),
    })
}

/// Compile a `.fl` file from disk, resolving imports.
pub fn compile_file(path: &Path, options: CompileOptions) -> Result<Compiled, CompileFailed> {
    if options.phase != CompilePhase::Full {
        let mut diagnostics = DiagnosticBag::new();
        diagnostics.push(FluxError::Parse {
            message: "compile_file requires CompilePhase::Full; use check_file for type-check only"
                .into(),
            span: span_to_miette(Span {
                source: SourceId(0),
                start: 0,
                end: 0,
            }),
            source_id: SourceId(0),
        });
        return Err(CompileFailed {
            source: SourceFile {
                id: SourceId(0),
                path: path.display().to_string(),
                contents: String::new(),
            },
            diagnostics,
        });
    }
    let mut diagnostics = DiagnosticBag::new();
    let Some(resolved) = resolve_file(path, &mut diagnostics) else {
        let source = SourceFile {
            id: SourceId(0),
            path: path.display().to_string(),
            contents: String::new(),
        };
        return Err(CompileFailed {
            source,
            diagnostics,
        });
    };
    let root = resolved.root.clone();
    let project_root = find_project_root(path);
    let cache_key = Some(CompileCache::cache_key_resolved(&resolved));
    let artifacts = lower_to_ir_resolved(
        root,
        &resolved,
        &options,
        cache_key,
        Some(project_root.as_path()),
        &mut diagnostics,
    )?;
    if diagnostics.has_errors() {
        return Err(CompileFailed {
            source: artifacts.source,
            diagnostics,
        });
    }
    let warnings = std::mem::take(&mut diagnostics.warnings);
    let Some(ir) = artifacts.ir else {
        return Err(CompileFailed {
            source: artifacts.source,
            diagnostics,
        });
    };
    let executor = Executor::with_mode(ir, options.mode).map_err(|msg| {
        diagnostics.push(FluxError::Parse {
            message: format!("VM validation failed: {msg}"),
            span: span_to_miette(Span {
                source: artifacts.source.id,
                start: 0,
                end: 0,
            }),
            source_id: artifacts.source.id,
        });
        CompileFailed {
            source: artifacts.source.clone(),
            diagnostics,
        }
    })?;
    Ok(Compiled {
        source: artifacts.source,
        hir: artifacts.hir,
        typed: artifacts.typed,
        executor,
        types: artifacts.types,
        warnings,
    })
}

/// Analyze a document for diagnostics / hover (follows imports when `path` is set).
pub fn analyze_document(path: Option<&Path>, contents: &str) -> Analyzed {
    let mut diagnostics = DiagnosticBag::new();
    let source_id = SourceId(0);

    let resolved_opt =
        path.and_then(|p| resolve_file_with_root(p, Some(contents), &mut diagnostics));

    let source_paths = resolved_opt
        .as_ref()
        .map(source_paths_from_resolved)
        .unwrap_or_default();

    let source = source_from(
        path.map(|p| p.display().to_string())
            .unwrap_or_else(|| "<buffer>".into()),
        contents,
    );

    let result = match resolved_opt {
        Some(resolved) => {
            let project_root = path.map(find_project_root);
            let cache_key = Some(CompileCache::cache_key_resolved(&resolved));
            lower_to_ir_resolved(
                source.clone(),
                &resolved,
                &CompileOptions::ide(),
                cache_key,
                project_root.as_deref(),
                &mut diagnostics,
            )
        }
        None => {
            let Some(ast) = parse_program(source_id, contents, &mut diagnostics) else {
                return Analyzed {
                    diagnostics,
                    hir: None,
                    typed: None,
                    source_paths,
                };
            };
            let cache_key = Some(CompileCache::hash_source(contents));
            lower_to_ir(
                source.clone(),
                ast,
                &CompileOptions::ide(),
                cache_key,
                None,
                &mut diagnostics,
            )
        }
    };

    match result {
        Ok(artifacts) => Analyzed {
            diagnostics,
            hir: Some(artifacts.hir),
            typed: Some(artifacts.typed),
            source_paths,
        },
        Err(failed) => {
            for err in failed.diagnostics.errors {
                diagnostics.push(err);
            }
            for warn in failed.diagnostics.warnings {
                diagnostics.push_warning(warn);
            }
            Analyzed {
                diagnostics,
                hir: None,
                typed: None,
                source_paths,
            }
        }
    }
}

fn source_paths_from_resolved(resolved: &ResolvedProgram) -> HashMap<SourceId, String> {
    resolved
        .modules
        .iter()
        .map(|m| (m.source.id, m.source.path.clone()))
        .collect()
}

fn lower_to_ir_resolved(
    source: SourceFile,
    resolved: &ResolvedProgram,
    options: &CompileOptions,
    cache_key: Option<String>,
    project_root: Option<&Path>,
    diagnostics: &mut DiagnosticBag,
) -> Result<Artifacts, CompileFailed> {
    let Some(hir) = lower_linked_program(resolved, diagnostics) else {
        return Err(CompileFailed {
            source,
            diagnostics: std::mem::take(diagnostics),
        });
    };

    finish_lower(source, hir, options, cache_key, project_root, diagnostics)
}

fn lower_to_ir(
    source: SourceFile,
    ast: Program,
    options: &CompileOptions,
    cache_key: Option<String>,
    project_root: Option<&Path>,
    diagnostics: &mut DiagnosticBag,
) -> Result<Artifacts, CompileFailed> {
    let Some(hir) = lower_program(&ast, diagnostics) else {
        return Err(CompileFailed {
            source,
            diagnostics: std::mem::take(diagnostics),
        });
    };

    finish_lower(source, hir, options, cache_key, project_root, diagnostics)
}

fn finish_lower(
    source: SourceFile,
    mut hir: HirProgram,
    options: &CompileOptions,
    cache_key: Option<String>,
    project_root: Option<&Path>,
    diagnostics: &mut DiagnosticBag,
) -> Result<Artifacts, CompileFailed> {
    let cached = if options.analysis_cache {
        cache_key.as_deref().and_then(get_analysis_cache)
    } else {
        None
    };
    let (hir, typed, types) = if let Some(hit) = cached {
        (hit.hir, hit.typed, hit.types)
    } else {
        flux_types::monomorphize_program(&mut hir, source.id, diagnostics);
        let Some(typed) = typecheck_program(&hir, source.id, diagnostics) else {
            return Err(CompileFailed {
                source,
                diagnostics: std::mem::take(diagnostics),
            });
        };
        if diagnostics.has_errors() {
            return Err(CompileFailed {
                source,
                diagnostics: std::mem::take(diagnostics),
            });
        }
        let types = build_type_map(&hir, &typed);

        if options.analysis_cache {
            if let Some(key) = cache_key.as_deref() {
                put_analysis_cache(
                    key,
                    CachedAnalysis {
                        hir: hir.clone(),
                        typed: typed.clone(),
                        types: types.clone(),
                    },
                );
            }
        }
        (hir, typed, types)
    };

    if options.phase == CompilePhase::Check {
        return Ok(Artifacts {
            source,
            hir,
            typed,
            types,
            ir: None,
        });
    }

    let ir = emit_ir(
        &source,
        &hir,
        options,
        cache_key.as_deref(),
        project_root,
        diagnostics,
    )?
    .ok_or_else(|| CompileFailed {
        source: source.clone(),
        diagnostics: std::mem::take(diagnostics),
    })?;

    Ok(Artifacts {
        source,
        hir,
        typed,
        types,
        ir: Some(ir),
    })
}

/// Lower HIR to IR, optionally reusing a validated on-disk cache entry.
fn emit_ir(
    source: &SourceFile,
    hir: &HirProgram,
    options: &CompileOptions,
    cache_key: Option<&str>,
    project_root: Option<&Path>,
    diagnostics: &mut DiagnosticBag,
) -> Result<Option<CompiledProgram>, CompileFailed> {
    let key = cache_key.unwrap_or(&source.contents);
    let root = resolve_project_root(
        project_root,
        if source.path.is_empty() || source.path == "<buffer>" {
            None
        } else {
            Some(Path::new(&source.path))
        },
    );
    if options.use_cache {
        if let Some(cached) = with_project_cache(&root, |cache| cache.get_program(key)) {
            return Ok(Some(cached));
        }
    }

    let Some(ir) = IrCodegen.emit(hir, source.id, diagnostics) else {
        return Ok(None);
    };

    if options.use_cache {
        store_cache(&root, key, &ir);
    }

    Ok(Some(ir))
}

fn store_cache(project_root: &Path, key: &str, ir: &CompiledProgram) {
    with_project_cache(project_root, |cache| {
        cache.put_program(key, ir);
    });
}

fn build_type_map(hir: &HirProgram, typed: &TypedProgram) -> HashMap<String, String> {
    hir.names
        .iter()
        .filter_map(|(id, name)| typed.types.get(id).map(|t| (name.clone(), t.display())))
        .collect()
}

/// Build a [`SourceFile`] from path + contents.
pub fn source_from(path: impl Into<String>, contents: impl Into<String>) -> SourceFile {
    SourceFile {
        id: SourceId(0),
        path: path.into(),
        contents: contents.into(),
    }
}
