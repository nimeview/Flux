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
use flux_resolve::{
    find_project_root, try_resolve_file_with_root, ResolveFailure, ResolvedProgram,
};
use flux_runtime::{ExecutionMode, Executor, ExecutorError};
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
    /// Every loaded module's source (root first); diagnostics render against these by id.
    pub sources: Vec<SourceFile>,
    pub hir: HirProgram,
    pub typed: TypedProgram,
    pub types: HashMap<String, String>,
    pub warnings: Vec<FluxWarning>,
}

/// Successful compilation result.
pub struct Compiled {
    pub source: SourceFile,
    /// Every loaded module's source (root first); diagnostics render against these by id.
    pub sources: Vec<SourceFile>,
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
    /// Every source read during analysis (root first), including the file a
    /// resolution failure points into, so diagnostics map to their own text.
    pub sources: Vec<SourceFile>,
}

/// Compilation failed — diagnostics are ready to render.
#[derive(Debug)]
pub struct CompileFailed {
    /// Every source read before the failure; the primary file is first.
    pub sources: Vec<SourceFile>,
    pub diagnostics: DiagnosticBag,
}

impl CompileFailed {
    fn new(primary: SourceFile, mut sources: Vec<SourceFile>, diagnostics: DiagnosticBag) -> Self {
        sources.retain(|s| s.id != primary.id);
        sources.insert(0, primary);
        Self {
            sources,
            diagnostics,
        }
    }

    /// The file the failure is reported for (the root, or the file the first
    /// resolution error points into).
    pub fn source(&self) -> &SourceFile {
        &self.sources[0]
    }
}

struct Artifacts {
    source: SourceFile,
    sources: Vec<SourceFile>,
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
        let sources = vec![source.clone()];
        return Err(CompileFailed::new(source, sources, diagnostics));
    };

    let artifacts = lower_to_ir(source, ast, &options, None, None, &mut diagnostics)?;
    if diagnostics.has_errors() {
        return Err(CompileFailed::new(
            artifacts.source,
            artifacts.sources,
            diagnostics,
        ));
    }

    let warnings = std::mem::take(&mut diagnostics.warnings);
    let Some(ir) = artifacts.ir else {
        return Err(CompileFailed::new(
            artifacts.source,
            artifacts.sources,
            diagnostics,
        ));
    };
    let executor = Executor::with_mode(ir, options.mode).map_err(|err| {
        diagnostics.push(executor_error_diagnostic(
            err,
            &artifacts.hir,
            &artifacts.source,
        ));
        CompileFailed::new(
            artifacts.source.clone(),
            artifacts.sources.clone(),
            diagnostics,
        )
    })?;
    Ok(Compiled {
        source: artifacts.source,
        sources: artifacts.sources,
        hir: artifacts.hir,
        typed: artifacts.typed,
        executor,
        types: artifacts.types,
        warnings,
    })
}

/// Turns an executor start-up failure into a diagnostic. Runtime errors point
/// at the binding being evaluated when it is in the rendered source.
fn executor_error_diagnostic(
    err: ExecutorError,
    hir: &HirProgram,
    source: &SourceFile,
) -> FluxError {
    let file_start = Span {
        source: source.id,
        start: 0,
        end: 0,
    };
    match err {
        ExecutorError::Invalid(msg) => FluxError::Parse {
            message: format!("VM validation failed: {msg}"),
            span: span_to_miette(file_start),
            source_id: source.id,
        },
        ExecutorError::Runtime(err) => {
            let span = err
                .binding
                .as_deref()
                .and_then(|name| binding_span(hir, name))
                .unwrap_or(file_start);
            FluxError::Runtime {
                message: err.to_string(),
                span: span_to_miette(span),
                source_id: span.source,
            }
        }
    }
}

fn binding_span(hir: &HirProgram, name: &str) -> Option<Span> {
    hir.items.iter().find_map(|item| match item {
        flux_hir::HirItem::Binding(b)
            if hir.names.get(&b.name).map(String::as_str) == Some(name) =>
        {
            Some(b.span)
        }
        _ => None,
    })
}

/// Builds the failure for a module-resolution error, attaching the source the
/// first error points into so it renders with the right file and snippet.
fn resolve_failed(
    path: &Path,
    failure: ResolveFailure,
    diagnostics: DiagnosticBag,
) -> CompileFailed {
    let source = diagnostics
        .errors
        .first()
        .and_then(|e| failure.source(e.source_id()))
        .cloned()
        .unwrap_or_else(|| SourceFile {
            id: SourceId(0),
            path: path.display().to_string(),
            contents: String::new(),
        });
    CompileFailed::new(source, failure.sources, diagnostics)
}

/// Type-check a `.fl` file from disk (parse → resolve → HIR → types; no IR or VM).
pub fn check_file(path: &Path) -> Result<Checked, CompileFailed> {
    let mut diagnostics = DiagnosticBag::new();
    let resolved = match try_resolve_file_with_root(path, None, &mut diagnostics) {
        Ok(resolved) => resolved,
        Err(failure) => return Err(resolve_failed(path, failure, diagnostics)),
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
        return Err(CompileFailed::new(
            artifacts.source,
            artifacts.sources,
            diagnostics,
        ));
    }
    Ok(Checked {
        source: artifacts.source,
        sources: artifacts.sources,
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
        let source = SourceFile {
            id: SourceId(0),
            path: path.display().to_string(),
            contents: String::new(),
        };
        return Err(CompileFailed::new(source, Vec::new(), diagnostics));
    }
    let mut diagnostics = DiagnosticBag::new();
    let resolved = match try_resolve_file_with_root(path, None, &mut diagnostics) {
        Ok(resolved) => resolved,
        Err(failure) => return Err(resolve_failed(path, failure, diagnostics)),
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
        return Err(CompileFailed::new(
            artifacts.source,
            artifacts.sources,
            diagnostics,
        ));
    }
    let warnings = std::mem::take(&mut diagnostics.warnings);
    let Some(ir) = artifacts.ir else {
        return Err(CompileFailed::new(
            artifacts.source,
            artifacts.sources,
            diagnostics,
        ));
    };
    let executor = Executor::with_mode(ir, options.mode).map_err(|err| {
        diagnostics.push(executor_error_diagnostic(
            err,
            &artifacts.hir,
            &artifacts.source,
        ));
        CompileFailed::new(
            artifacts.source.clone(),
            artifacts.sources.clone(),
            diagnostics,
        )
    })?;
    Ok(Compiled {
        source: artifacts.source,
        sources: artifacts.sources,
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

    let mut failed_sources = Vec::new();
    let resolved_opt = path.and_then(|p| {
        try_resolve_file_with_root(p, Some(contents), &mut diagnostics)
            .map_err(|failure| failed_sources = failure.sources)
            .ok()
    });
    // A resolution failure still has diagnostics; don't re-run single-file
    // analysis over them, which would duplicate or misattribute errors.
    if path.is_some() && resolved_opt.is_none() {
        return Analyzed {
            diagnostics,
            hir: None,
            typed: None,
            source_paths: failed_sources
                .iter()
                .map(|s| (s.id, s.path.clone()))
                .collect(),
            sources: failed_sources,
        };
    }

    let source_paths = resolved_opt
        .as_ref()
        .map(source_paths_from_resolved)
        .unwrap_or_default();
    let resolved_sources: Vec<SourceFile> = resolved_opt
        .as_ref()
        .map(|r| r.modules.iter().map(|m| m.source.clone()).collect())
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
                    sources: vec![source],
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
            sources: artifacts.sources,
        },
        Err(failed) => {
            for err in failed.diagnostics.errors {
                diagnostics.push(err);
            }
            for warn in failed.diagnostics.warnings {
                diagnostics.push_warning(warn);
            }
            let sources = if failed.sources.is_empty() {
                resolved_sources
            } else {
                failed.sources
            };
            Analyzed {
                diagnostics,
                hir: None,
                typed: None,
                source_paths,
                sources,
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
    let sources: Vec<SourceFile> = resolved.modules.iter().map(|m| m.source.clone()).collect();
    let Some(hir) = lower_linked_program(resolved, diagnostics) else {
        return Err(CompileFailed::new(
            source,
            sources,
            std::mem::take(diagnostics),
        ));
    };

    finish_lower(
        source,
        sources,
        hir,
        options,
        cache_key,
        project_root,
        diagnostics,
    )
}

fn lower_to_ir(
    source: SourceFile,
    ast: Program,
    options: &CompileOptions,
    cache_key: Option<String>,
    project_root: Option<&Path>,
    diagnostics: &mut DiagnosticBag,
) -> Result<Artifacts, CompileFailed> {
    let sources = vec![source.clone()];
    let Some(hir) = lower_program(&ast, diagnostics) else {
        return Err(CompileFailed::new(
            source,
            sources,
            std::mem::take(diagnostics),
        ));
    };

    finish_lower(
        source,
        sources,
        hir,
        options,
        cache_key,
        project_root,
        diagnostics,
    )
}

fn finish_lower(
    source: SourceFile,
    sources: Vec<SourceFile>,
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
            return Err(CompileFailed::new(
                source,
                sources,
                std::mem::take(diagnostics),
            ));
        };
        if diagnostics.has_errors() {
            return Err(CompileFailed::new(
                source,
                sources,
                std::mem::take(diagnostics),
            ));
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
            sources,
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
    .ok_or_else(|| {
        CompileFailed::new(source.clone(), sources.clone(), std::mem::take(diagnostics))
    })?;

    Ok(Artifacts {
        source,
        sources,
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
    // Never key on raw source text: it becomes a file name.
    let source_key;
    let key = match cache_key {
        Some(key) => key,
        None => {
            source_key = CompileCache::hash_source(&source.contents);
            &source_key
        }
    };
    let root = resolve_project_root(
        project_root,
        if source.path.is_empty() || source.path == "<buffer>" {
            None
        } else {
            Some(Path::new(&source.path))
        },
    );
    if options.use_cache {
        // An entry that decodes but fails IR validation is dropped and rebuilt.
        let cached = with_project_cache(&root, |cache| {
            let program = cache.get_program(key)?;
            if flux_runtime::validate_program(&program).is_ok() {
                return Some(program);
            }
            cache.invalidate(key);
            None
        });
        if let Some(cached) = cached {
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
