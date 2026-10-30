//! Module resolution for multi-file Flux programs.

mod manifest;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceFile, SourceId};
use flux_std::{export_in_std_module, is_std_module, std_module_name};
use flux_syntax::{
    has_module_extension, parse_program, with_module_extension, ImportDecl, Item, Program,
};

pub use manifest::{
    load_path_dependencies, parse_manifest, DependencySpec, PackageManifest, PackageSection,
    PathDependency, PathsSection,
};

/// A resolved multi-module program ready for HIR lowering.
#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    pub root: SourceFile,
    pub modules: Vec<LoadedModule>,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub path: PathBuf,
    pub source: SourceFile,
    pub ast: Program,
    pub imports: Vec<ImportDecl>,
}

/// Import binding after validation — ready for HIR lowering.
#[derive(Debug, Clone)]
pub struct BoundImport {
    pub alias: String,
    pub module_index: usize,
    pub symbols: Vec<String>,
    pub span: flux_diagnostics::Span,
}

/// Resolved program with validated import graph (replaces flat `merge_modules`).
#[derive(Debug)]
pub struct LinkedProgram {
    pub root: SourceFile,
    pub modules: Vec<LoadedModule>,
    pub imports: Vec<BoundImport>,
}

/// Load and resolve a `.fl` file and its transitive imports.
pub fn resolve_file(path: &Path, diagnostics: &mut DiagnosticBag) -> Option<ResolvedProgram> {
    resolve_file_with_root(path, None, diagnostics)
}

/// Load and resolve a `.fl` file and its transitive imports, using `root_contents`
/// in place of the on-disk contents of `path` if provided. Imported modules are still
/// read from disk. This lets editor tooling (e.g. the LSP server) resolve imports for
/// an unsaved buffer without losing the in-memory edits to the root file.
pub fn resolve_file_with_root(
    path: &Path,
    root_contents: Option<&str>,
    diagnostics: &mut DiagnosticBag,
) -> Option<ResolvedProgram> {
    try_resolve_file_with_root(path, root_contents, diagnostics).ok()
}

/// Sources that were read before resolution failed, so callers can render
/// diagnostics against the file they point into (including imported modules).
#[derive(Debug, Clone, Default)]
pub struct ResolveFailure {
    pub sources: Vec<SourceFile>,
}

impl ResolveFailure {
    /// The source a diagnostic with `id` belongs to, if it was read.
    pub fn source(&self, id: SourceId) -> Option<&SourceFile> {
        self.sources.iter().find(|s| s.id == id)
    }
}

/// Like [`resolve_file_with_root`], but on failure returns every source read so far.
/// Failure always leaves at least one error in `diagnostics`.
pub fn try_resolve_file_with_root(
    path: &Path,
    root_contents: Option<&str>,
    diagnostics: &mut DiagnosticBag,
) -> Result<ResolvedProgram, ResolveFailure> {
    let mut loader = ModuleLoader::new(path);
    let loaded = loader.load_recursive(path, root_contents, None, diagnostics);
    match (loaded, loader.modules.first()) {
        (Some(()), Some(root)) => Ok(ResolvedProgram {
            root: root.source.clone(),
            modules: loader.modules,
        }),
        _ => {
            if !diagnostics.has_errors() {
                diagnostics.push(FluxError::Parse {
                    message: format!("failed to resolve {}", path.display()),
                    span: span_to_miette(flux_diagnostics::Span::DUMMY),
                    source_id: SourceId(0),
                });
            }
            let mut sources: Vec<SourceFile> =
                loader.modules.into_iter().map(|m| m.source).collect();
            sources.extend(loader.failed_source);
            Err(ResolveFailure { sources })
        }
    }
}

struct ModuleLoader {
    modules: Vec<LoadedModule>,
    seen: HashSet<PathBuf>,
    loading_stack: Vec<PathBuf>,
    project_root: PathBuf,
    path_deps: HashMap<String, PathBuf>,
    next_id: u32,
    /// Source whose parse failed; it never becomes a [`LoadedModule`].
    failed_source: Option<SourceFile>,
}

impl ModuleLoader {
    fn new(root_path: &Path) -> Self {
        let project_root = find_project_root(root_path);
        let path_deps = load_path_dependencies(&project_root)
            .into_iter()
            .map(|d| (d.name, d.module_root))
            .collect();
        Self {
            modules: Vec::new(),
            seen: HashSet::new(),
            loading_stack: Vec::new(),
            project_root,
            path_deps,
            next_id: 0,
            failed_source: None,
        }
    }

    fn alloc_id(&mut self) -> SourceId {
        let id = SourceId(self.next_id);
        self.next_id += 1;
        id
    }

    /// `import_site` is the `import` that requested `path` (none for the root);
    /// cycle and read errors point there because `path` may have no source.
    fn load_recursive(
        &mut self,
        path: &Path,
        override_contents: Option<&str>,
        import_site: Option<flux_diagnostics::Span>,
        diagnostics: &mut DiagnosticBag,
    ) -> Option<()> {
        let site = import_site.unwrap_or(flux_diagnostics::Span::DUMMY);
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if self.loading_stack.contains(&canonical) {
            let cycle = self
                .loading_stack
                .iter()
                .chain(std::iter::once(&canonical))
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            diagnostics.push(FluxError::CyclicImport {
                cycle,
                span: span_to_miette(site),
                source_id: site.source,
            });
            return None;
        }
        if self.seen.contains(&canonical) {
            return Some(());
        }
        self.seen.insert(canonical.clone());
        self.loading_stack.push(canonical.clone());

        let contents = match override_contents {
            Some(c) => c.to_string(),
            None => match fs::read_to_string(&canonical) {
                Ok(c) => c,
                Err(e) => {
                    diagnostics.push(FluxError::Parse {
                        message: format!("failed to read {}: {e}", canonical.display()),
                        span: span_to_miette(site),
                        source_id: site.source,
                    });
                    return None;
                }
            },
        };

        let source_id = self.alloc_id();
        let source = SourceFile {
            id: source_id,
            path: canonical.display().to_string(),
            contents: contents.clone(),
        };

        let mut local_diag = DiagnosticBag::new();
        let ast = parse_program(source_id, &contents, &mut local_diag);
        let failed = ast.is_none() || local_diag.has_errors();
        diagnostics.errors.extend(local_diag.errors);
        diagnostics.warnings.extend(local_diag.warnings);
        let Some(ast) = ast.filter(|_| !failed) else {
            self.failed_source = Some(source);
            return None;
        };

        let imports = extract_imports(&ast);
        let module = LoadedModule {
            path: canonical.clone(),
            source,
            ast,
            imports: imports.clone(),
        };
        self.modules.push(module);

        let base_dir = canonical.parent()?;
        for import in imports {
            if std_module_name(&import.path).is_some()
                && !local_module_exists(base_dir, &import.path)
            {
                continue;
            }
            let Some(import_path) = resolve_import_path(
                base_dir,
                &import.path,
                &self.project_root,
                &self.path_deps,
                &import,
                diagnostics,
            ) else {
                self.loading_stack.pop();
                return None;
            };
            if !import_path.exists() {
                diagnostics.push(FluxError::Parse {
                    message: format!("module not found: {}", import.path),
                    span: span_to_miette(import.span),
                    source_id: import.span.source,
                });
                self.loading_stack.pop();
                return None;
            }
            self.load_recursive(&import_path, None, Some(import.span), diagnostics)?;
        }
        self.loading_stack.pop();
        Some(())
    }
}

fn extract_imports(program: &Program) -> Vec<ImportDecl> {
    program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Import(imp) => Some(imp.clone()),
            _ => None,
        })
        .collect()
}

/// Walk upward from `start` to find the directory containing `flux.toml`.
pub fn find_project_root(start: &Path) -> PathBuf {
    let mut dir = start
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    loop {
        if dir.join("flux.toml").exists() {
            return dir;
        }
        if !dir.pop() {
            return start
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
        }
    }
}

fn resolve_import_path(
    base_dir: &Path,
    import_path: &str,
    project_root: &Path,
    path_deps: &HashMap<String, PathBuf>,
    import: &ImportDecl,
    diagnostics: &mut DiagnosticBag,
) -> Option<PathBuf> {
    if import_path.contains("..")
        || import_path
            .split('.')
            .any(|segment| segment == ".." || segment.contains(".."))
    {
        diagnostics.push(FluxError::ImportEscape {
            path: import_path.to_string(),
            span: span_to_miette(import.span),
            source_id: import.span.source,
        });
        return None;
    }

    let segments: Vec<&str> = import_path.split('.').collect();
    let resolved = if let Some(dep_root) = segments.first().and_then(|s| path_deps.get(*s)) {
        let rest = segments[1..].join(std::path::MAIN_SEPARATOR_STR);
        if rest.is_empty() {
            dep_root.join(with_module_extension("lib"))
        } else if has_module_extension(&rest) {
            dep_root.join(&rest)
        } else {
            dep_root.join(with_module_extension(&rest))
        }
    } else {
        let path = import_path.replace('.', std::path::MAIN_SEPARATOR_STR);
        let with_ext = if has_module_extension(&path) {
            path
        } else {
            with_module_extension(&path)
        };
        let candidate = if is_std_module(import_path) && segments.len() == 1 {
            let local = base_dir.join(&with_ext);
            if local.exists() {
                local
            } else {
                project_root.join("stdlib").join(with_ext)
            }
        } else if segments.first() == Some(&"stdlib") {
            let rest: Vec<&str> = segments[1..].to_vec();
            let rest_path = if rest.is_empty() {
                "prelude".to_string()
            } else {
                rest.join(std::path::MAIN_SEPARATOR_STR)
            };
            let rest_ext = if has_module_extension(&rest_path) {
                rest_path
            } else {
                with_module_extension(&rest_path)
            };
            project_root.join("stdlib").join(rest_ext)
        } else {
            base_dir.join(with_ext)
        };
        candidate
    };

    let Ok(canonical) = resolved.canonicalize() else {
        diagnostics.push(FluxError::Parse {
            message: format!("module not found: {import_path}"),
            span: span_to_miette(import.span),
            source_id: import.span.source,
        });
        return None;
    };
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let within_project = canonical.starts_with(&root);
    let within_dep = path_deps.values().any(|dep| {
        dep.canonicalize()
            .map(|c| canonical.starts_with(&c))
            .unwrap_or_else(|_| canonical.starts_with(dep))
    });
    if !within_project && !within_dep {
        diagnostics.push(FluxError::ImportEscape {
            path: import_path.to_string(),
            span: span_to_miette(import.span),
            source_id: import.span.source,
        });
        return None;
    }
    Some(canonical)
}

/// Validate imports and build a linked program for qualified-name lowering.
pub fn link_modules(
    resolved: &ResolvedProgram,
    diagnostics: &mut DiagnosticBag,
) -> Option<LinkedProgram> {
    let root_mod = resolved.modules.first()?;
    let base_dir = root_mod.path.parent()?;
    let project_root = find_project_root(&root_mod.path);
    let path_deps: HashMap<String, PathBuf> = load_path_dependencies(&project_root)
        .into_iter()
        .map(|d| (d.name, d.module_root))
        .collect();

    let path_index: std::collections::HashMap<PathBuf, usize> = resolved
        .modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.path.clone(), i))
        .collect();

    let mut imports = Vec::new();
    for import in extract_imports(&root_mod.ast) {
        if std_module_name(&import.path).is_some() && !local_module_exists(base_dir, &import.path) {
            continue;
        }
        let alias = import.alias.clone().unwrap_or_else(|| {
            import
                .path
                .split('.')
                .next_back()
                .unwrap_or(&import.path)
                .to_string()
        });

        let import_path = resolve_import_path(
            base_dir,
            &import.path,
            &project_root,
            &path_deps,
            &import,
            diagnostics,
        )?;
        let Some(&module_index) = path_index.get(&import_path) else {
            diagnostics.push(FluxError::Parse {
                message: format!("module not found: {}", import.path),
                span: span_to_miette(import.span),
                source_id: import.span.source,
            });
            return None;
        };

        let module = &resolved.modules[module_index];
        if let Some(std_name) = std_module_name(&import.path) {
            if !import.symbols.is_empty() {
                for sym in &import.symbols {
                    if !export_in_std_module(std_name, sym) {
                        diagnostics.push(FluxError::UnknownExport {
                            module: import.path.clone(),
                            name: sym.clone(),
                            span: span_to_miette(import.span),
                            source_id: import.span.source,
                        });
                        return None;
                    }
                }
            }
        } else if import.symbols.is_empty() {
            for export in pub_export_names(&module.ast) {
                if !export_exists(&module.ast, &export) {
                    diagnostics.push(FluxError::UnknownExport {
                        module: import.path.clone(),
                        name: export,
                        span: span_to_miette(import.span),
                        source_id: import.span.source,
                    });
                    return None;
                }
            }
        } else {
            for sym in &import.symbols {
                if !is_pub_export(&module.ast, sym) {
                    diagnostics.push(FluxError::UnknownExport {
                        module: import.path.clone(),
                        name: sym.clone(),
                        span: span_to_miette(import.span),
                        source_id: import.span.source,
                    });
                    return None;
                }
            }
        }

        imports.push(BoundImport {
            alias,
            module_index,
            symbols: import.symbols,
            span: import.span,
        });
    }

    Some(LinkedProgram {
        root: resolved.root.clone(),
        modules: resolved.modules.clone(),
        imports,
    })
}

fn local_module_exists(base_dir: &Path, import_path: &str) -> bool {
    let path = import_path.replace('.', std::path::MAIN_SEPARATOR_STR);
    base_dir.join(with_module_extension(&path)).exists()
}

fn pub_export_names(program: &Program) -> Vec<String> {
    program
        .items
        .iter()
        .filter_map(export_name_if_pub)
        .collect()
}

fn export_name_if_pub(item: &Item) -> Option<String> {
    match item {
        Item::Fn(f) if f.pub_ => Some(f.name.name.clone()),
        Item::RFn(f) if f.pub_ => Some(f.name.name.clone()),
        Item::Struct(s) if s.pub_ => Some(s.name.name.clone()),
        Item::Enum(e) if e.pub_ => Some(e.name.name.clone()),
        Item::Trait(t) if t.pub_ => Some(t.name.name.clone()),
        Item::Event(e) if e.pub_ => Some(e.name.name.clone()),
        Item::Let(l) if l.pub_ => l.pattern.primary_name().map(|n| n.to_string()),
        Item::Derived(d) if d.pub_ => d.pattern.primary_name().map(|n| n.to_string()),
        Item::Const(c) if c.pub_ => Some(c.name.name.clone()),
        _ => None,
    }
}

fn export_exists(program: &Program, name: &str) -> bool {
    program
        .items
        .iter()
        .any(|item| export_name_if_pub(item).as_deref() == Some(name))
}

fn is_pub_export(program: &Program, name: &str) -> bool {
    export_exists(program, name)
}

/// Root module AST with import declarations removed.
pub fn root_program(linked: &LinkedProgram) -> Program {
    let root_mod = linked.modules.first().expect("root module");
    let items: Vec<Item> = root_mod
        .ast
        .items
        .iter()
        .filter(|item| !matches!(item, Item::Import(_)))
        .cloned()
        .collect();
    Program {
        source: root_mod.ast.source,
        items,
    }
}

/// Merge all module ASTs into a single program (imports elided, symbols prefixed).
/// Deprecated: use [`link_modules`] + [`lower_linked_program`] instead.
pub fn merge_modules(resolved: &ResolvedProgram) -> Program {
    let root_mod = resolved.modules.first().expect("at least root module");
    let mut items: Vec<Item> = root_mod
        .ast
        .items
        .iter()
        .filter(|item| !matches!(item, Item::Import(_)))
        .cloned()
        .collect();

    let mut imported_items = Vec::new();
    for import in &root_mod.imports {
        let prefix = import.path.split('.').next_back().unwrap_or(&import.path);
        for mod_ in resolved.modules.iter().skip(1) {
            let matches = mod_.path.file_stem().map(|s| s == prefix).unwrap_or(false);
            if matches {
                reexport_symbols(&mut imported_items, &mod_.ast, prefix, &import.symbols);
            }
        }
    }

    imported_items.append(&mut items);

    Program {
        source: root_mod.ast.source,
        items: imported_items,
    }
}

fn reexport_symbols(items: &mut Vec<Item>, module: &Program, prefix: &str, symbols: &[String]) {
    let prefix_name = prefix.split('.').next_back().unwrap_or(prefix);
    for item in &module.items {
        match item {
            Item::Fn(f) if symbols.is_empty() || symbols.contains(&f.name.name) => {
                let mut renamed = f.clone();
                renamed.name.name = format!("{prefix_name}_{}", f.name.name);
                items.push(Item::Fn(renamed));
            }
            Item::Let(l)
                if symbols.is_empty()
                    || l.pattern
                        .primary_name()
                        .is_some_and(|n| symbols.contains(&n.to_string())) =>
            {
                let mut renamed = l.clone();
                if let Some(name) = renamed.pattern.primary_name() {
                    renamed.pattern = flux_syntax::LetPattern::Ident(flux_syntax::Ident {
                        name: format!("{}_{}", prefix_name, name),
                        span: renamed.pattern.span(),
                    });
                }
                items.push(Item::Let(renamed));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_import_math_integration() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../programs/ok/modules/import_math.fl");
        let mut diag = DiagnosticBag::new();
        let resolved = resolve_file(&path, &mut diag).expect("resolve");
        assert_eq!(resolved.modules.len(), 2, "diag: {:?}", diag.errors);
        let linked = link_modules(&resolved, &mut diag).expect("link");
        assert_eq!(linked.imports.len(), 1);
        assert_eq!(linked.imports[0].alias, "m");
    }

    #[test]
    fn resolves_path_dependency_import() {
        let root = std::env::temp_dir().join(format!("flux_path_dep_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("libs/utils")).unwrap();
        std::fs::write(
            root.join("flux.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"
entry = "main.fl"

[dependencies]
utils = { path = "libs/utils" }
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("libs/utils/flux.toml"),
            r#"
[package]
name = "utils"
version = "0.1.0"
entry = "lib.fl"

[paths]
src = "."
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("libs/utils/helpers.fl"),
            "pub fn twice(x: Int) -> Int { x * 2 }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("main.fl"),
            "import utils.helpers as h\n\nlet result = h.twice(3)\n",
        )
        .unwrap();

        let mut diag = DiagnosticBag::new();
        let resolved = resolve_file(&root.join("main.fl"), &mut diag).expect("resolve path dep");
        assert_eq!(resolved.modules.len(), 2, "diag: {:?}", diag.errors);
        let linked = link_modules(&resolved, &mut diag).expect("link");
        assert_eq!(linked.imports.len(), 1);
        assert_eq!(linked.imports[0].alias, "h");

        let _ = std::fs::remove_dir_all(&root);
    }
}
