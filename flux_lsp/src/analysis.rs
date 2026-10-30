//! Shared compile-and-inspect pipeline used by both the legacy JSON diagnostics
//! command (`flux lsp <file>`) and the stdio language server.

use std::collections::HashMap;
use std::path::Path;

use flux_diagnostics::{SourceFile, SourceId, Span};

use crate::position::byte_range_to_lsp;
pub use crate::position::position_to_offset;
use flux_driver::analyze_document as driver_analyze;
use flux_hir::{
    build_reference_index, rename_spans, symbol_at_offset, HirItem, HirProgram, ReferenceIndex,
    SymbolId,
};
use flux_types::{Type, TypeCheckContext, TypedProgram};

/// Severity for a ranged diagnostic (maps to LSP `DiagnosticSeverity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Error,
    Warning,
}

/// A diagnostic with a precise start/end range, ready to convert to LSP types.
/// Lines and columns are 1-based; columns count UTF-16 code units.
#[derive(Debug, Clone)]
pub struct RangedDiagnostic {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub message: String,
    pub code: String,
    pub level: DiagnosticLevel,
    /// File the range is in; `None` means the analyzed document itself.
    pub path: Option<String>,
}

/// Definition site for a symbol, keyed by file path and source span.
#[derive(Debug, Clone)]
pub struct SymbolLocation {
    pub path: String,
    pub span: Span,
}

/// Index of symbol definitions for go-to-definition.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    /// Bare name → definition (module-level symbols and re-exports).
    pub by_name: HashMap<String, SymbolLocation>,
    /// `(module_alias, export_name)` → definition for qualified imports.
    pub qualified: HashMap<(String, String), SymbolLocation>,
    /// Symbol id → definition for impl methods and mangled imports.
    pub by_id: HashMap<SymbolId, SymbolLocation>,
}

/// Result of compiling a single document (following its imports on disk, if any).
#[derive(Clone, Debug)]
pub struct DocAnalysis {
    pub diagnostics: Vec<RangedDiagnostic>,
    pub hir: Option<HirProgram>,
    pub typed: Option<TypedProgram>,
    pub source_paths: HashMap<SourceId, String>,
    pub symbols: SymbolIndex,
    pub references: Option<ReferenceIndex>,
}

/// Analyze a document's contents. `path` is used to resolve `import` statements
/// against sibling files on disk; when `None` (or when resolution fails), falls
/// back to treating `contents` as a standalone single-file program.
pub fn analyze_document(path: Option<&Path>, contents: &str) -> DocAnalysis {
    let analyzed = driver_analyze(path, contents);
    let fallback_path = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<buffer>".to_string());
    let hir = analyzed.hir.as_ref();
    let symbols = hir
        .map(|h| build_symbol_index(h, &analyzed.source_paths, &fallback_path))
        .unwrap_or_default();
    let references = hir.map(build_reference_index);
    DocAnalysis {
        diagnostics: to_ranged(&analyzed.diagnostics, contents, &analyzed.sources),
        hir: analyzed.hir,
        typed: analyzed.typed,
        source_paths: analyzed.source_paths,
        symbols,
        references,
    }
}

/// Build a symbol index from a lowered HIR program and multi-file source paths.
pub fn build_symbol_index(
    hir: &HirProgram,
    source_paths: &HashMap<SourceId, String>,
    fallback_path: &str,
) -> SymbolIndex {
    let mut index = SymbolIndex::default();

    for item in &hir.items {
        if let Some((id, span)) = item_symbol(item) {
            if let Some(loc) = location_for_span(source_paths, span, fallback_path) {
                index.by_id.insert(id, loc.clone());
                if let Some(name) = hir.names.get(&id) {
                    if !name.starts_with("__") {
                        index.by_name.insert(name.clone(), loc);
                    } else if let Some(bare) = demangle_mod_name(name) {
                        index.by_name.insert(bare, loc);
                    }
                }
            }
        }
    }

    for ((alias, export), id) in &hir.qualified_symbols {
        if let Some(loc) = index.by_id.get(id).cloned() {
            index.qualified.insert((alias.clone(), export.clone()), loc);
        }
    }

    index
}

fn item_symbol(item: &HirItem) -> Option<(SymbolId, Span)> {
    match item {
        HirItem::Binding(b) => Some((b.name, b.span)),
        HirItem::Fn(f) => Some((f.name, f.span)),
        HirItem::Event(e) => Some((e.name, e.span)),
        HirItem::Struct(s) => Some((s.name, s.span)),
        HirItem::Enum(e) => Some((e.name, e.span)),
        HirItem::Trait(t) => Some((t.name, t.span)),
        _ => None,
    }
}

fn demangle_mod_name(mangled: &str) -> Option<String> {
    let rest = mangled.strip_prefix("__mod_")?;
    rest.rsplit_once("__").map(|(_, name)| name.to_string())
}

fn location_for_span(
    source_paths: &HashMap<SourceId, String>,
    span: Span,
    fallback_path: &str,
) -> Option<SymbolLocation> {
    let path = source_paths
        .get(&span.source)
        .cloned()
        .unwrap_or_else(|| fallback_path.to_string());
    Some(SymbolLocation { path, span })
}

/// Resolve go-to-definition for a cursor position in `contents`.
pub fn definition_at_position(
    analysis: &DocAnalysis,
    contents: &str,
    line: u32,
    character: u32,
) -> Option<SymbolLocation> {
    let offset = position_to_offset(contents, line, character);
    let word = word_at_offset(contents, offset)?;

    if let Some((alias, member)) = qualified_member_before_dot(contents, offset) {
        return analysis
            .symbols
            .qualified
            .get(&(alias.to_string(), member.to_string()))
            .cloned()
            .or_else(|| analysis.symbols.by_name.get(member).cloned());
    }

    if dot_member_access(contents, offset) {
        return member_definition(analysis, contents, offset, word);
    }

    analysis.symbols.by_name.get(word).cloned()
}

fn dot_member_access(contents: &str, offset: usize) -> bool {
    let before = contents[..offset.min(contents.len())].trim_end();
    if before.ends_with('.') {
        return true;
    }
    // Cursor may be sitting on the `.` character.
    if offset < contents.len() && contents.as_bytes()[offset] == b'.' {
        let prefix = contents[..offset].trim_end();
        return !prefix.is_empty();
    }
    false
}

fn qualified_member_before_dot(contents: &str, offset: usize) -> Option<(&str, &str)> {
    let before = contents[..offset.min(contents.len())].trim_end();
    let before = before.strip_suffix('.')?.trim_end();
    let alias_end = before.len();
    let alias_start = before
        .char_indices()
        .rev()
        .find(|(_, c)| !is_ident(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let alias = &before[alias_start..alias_end];
    if alias.is_empty() || !alias.chars().all(is_ident) {
        return None;
    }
    let member = word_at_offset(contents, offset)?;
    Some((alias, member))
}

fn member_definition(
    analysis: &DocAnalysis,
    contents: &str,
    offset: usize,
    member: &str,
) -> Option<SymbolLocation> {
    let hir = analysis.hir.as_ref()?;
    let typed = analysis.typed.as_ref()?;
    let receiver = receiver_expr_before_dot(contents, offset)?;
    let receiver_ty = type_of_receiver(receiver, hir, typed)?;
    let inner = unwrap_type(&receiver_ty);
    let ctx = TypeCheckContext::from_program(hir);

    if let Type::Struct { name, .. } = inner {
        let ctx = TypeCheckContext::from_program(hir);
        if let Some(Type::Struct { fields, .. }) = ctx.user_type(&name) {
            if fields.iter().any(|(f, _)| f == member) {
                return analysis.symbols.by_name.get(&name).cloned();
            }
        }
        return None;
    }

    let type_tag = inner.display();
    let fn_id = ctx.trait_impl(&type_tag, member)?;
    analysis.symbols.by_id.get(&fn_id).cloned()
}

/// Member completion items after `.` from struct fields and trait impl methods.
pub fn member_completions(
    analysis: &DocAnalysis,
    contents: &str,
    line: u32,
    character: u32,
) -> Vec<MemberCompletion> {
    let offset = position_to_offset(contents, line, character);
    if !dot_member_access(contents, offset) {
        return Vec::new();
    }

    let patched = patch_incomplete_member_access(contents, offset);
    let fresh = if analysis.typed.is_none() && patched != contents {
        Some(analyze_document(None, &patched))
    } else {
        None
    };
    let analysis = fresh.as_ref().unwrap_or(analysis);

    let Some(hir) = analysis.hir.as_ref() else {
        return Vec::new();
    };
    let Some(typed) = analysis.typed.as_ref() else {
        return Vec::new();
    };

    let Some(receiver) = receiver_expr_before_dot(contents, offset)
        .or_else(|| receiver_expr_before_dot(&patched, offset))
    else {
        return Vec::new();
    };
    let Some(receiver_ty) = type_of_receiver(receiver, hir, typed) else {
        return Vec::new();
    };
    let inner = unwrap_type(&receiver_ty);
    let ctx = TypeCheckContext::from_program(hir);
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Type::Struct { name, .. } = &inner {
        let fields = ctx
            .user_type(name)
            .and_then(|t| match t {
                Type::Struct { fields, .. } => Some(fields.clone()),
                _ => None,
            })
            .unwrap_or_default();
        for (field, ty) in fields {
            if seen.insert(field.clone()) {
                items.push(MemberCompletion {
                    label: field.clone(),
                    detail: Some(ty.display()),
                    kind: MemberKind::Field,
                });
            }
        }
    }

    let type_tag = inner.display();
    for ((tag, method), sym_id) in &hir.trait_impls {
        if tag != &type_tag || !seen.insert(method.clone()) {
            continue;
        }
        let detail = typed.types.get(sym_id).map(describe_type);
        items.push(MemberCompletion {
            label: method.clone(),
            detail,
            kind: MemberKind::Method,
        });
    }

    let _ = ctx;
    items
}

#[derive(Debug, Clone)]
pub struct MemberCompletion {
    pub label: String,
    pub detail: Option<String>,
    pub kind: MemberKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    Field,
    Method,
}

fn patch_incomplete_member_access(contents: &str, offset: usize) -> String {
    let Some(_receiver) = receiver_expr_before_dot(contents, offset) else {
        return contents.to_string();
    };
    let lines: Vec<&str> = contents.lines().collect();
    let mut line_idx = 0usize;
    let mut pos = 0usize;
    for (i, line) in lines.iter().enumerate() {
        let next = pos + line.len() + 1;
        if offset < next {
            line_idx = i;
            break;
        }
        pos = next;
    }
    let mut out = lines[..line_idx].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str("__flux_sink = 0");
    if line_idx + 1 < lines.len() {
        out.push('\n');
        out.push_str(&lines[line_idx + 1..].join("\n"));
    }
    out.push('\n');
    out
}

fn receiver_expr_before_dot(contents: &str, offset: usize) -> Option<&str> {
    let mut end = offset.min(contents.len());
    if end < contents.len() && contents.as_bytes()[end] == b'.' {
        end += 1;
    }
    let before = contents[..end].trim_end();
    let before = before.strip_suffix('.')?.trim_end();
    if before.is_empty() {
        return None;
    }
    if before.ends_with(')') {
        let open = before.rfind('(')?;
        return Some(&before[open..]);
    }
    let end = before.len();
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| !is_ident(*c) && *c != '.')
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    Some(before[start..end].trim())
}

fn type_of_receiver(receiver: &str, hir: &HirProgram, typed: &TypedProgram) -> Option<Type> {
    if receiver.starts_with('(') {
        return Some(Type::Int);
    }
    let name = receiver.rsplit('.').next()?;
    let sym = hir
        .names
        .iter()
        .find(|(_, n)| n.as_str() == name)
        .map(|(id, _)| *id)?;
    typed.types.get(&sym).cloned()
}

fn unwrap_type(ty: &Type) -> Type {
    match ty {
        Type::Cell(inner) | Type::Read(inner) => *inner.clone(),
        other => other.clone(),
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Convert a [`SymbolLocation`] span to 0-based LSP line/column range.
pub fn span_to_lsp_range(
    contents: &str,
    span: &Span,
    source_paths: &HashMap<SourceId, String>,
) -> Option<(u32, u32, u32, u32)> {
    // The analyzed document is SourceId 0; use its (possibly unsaved) buffer.
    let file_contents = if span.source == SourceId(0) {
        None
    } else {
        let path = source_paths.get(&span.source)?;
        std::fs::read_to_string(path).ok()
    };
    let text = file_contents.as_deref().unwrap_or(contents);
    Some(byte_range_to_lsp(text, span.start, span.end))
}

/// A friendly, one-line description of `symbol`'s type for hover text.
pub fn describe_type(ty: &Type) -> String {
    match ty {
        Type::Cell(inner) => format!("source {}", inner.display()),
        Type::Read(inner) => format!("derived {}", inner.display()),
        Type::Event(inner) if matches!(inner.as_ref(), Type::Unit) => "event".to_string(),
        Type::Event(inner) => format!("event({})", inner.display()),
        Type::Func { params, ret } => {
            let ps = params
                .iter()
                .map(Type::display)
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn({ps}) -> {}", ret.display())
        }
        other => other.display(),
    }
}

/// Looks up a symbol by name in the HIR's name table and returns its hover text,
/// preferring module-level items so a binding's own name resolves to itself
/// rather than to an unrelated inner local that happens to share the name.
pub fn hover_for_name(
    hir: &HirProgram,
    typed: Option<&TypedProgram>,
    word: &str,
) -> Option<String> {
    let module_level: std::collections::HashSet<&str> = hir
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Binding(b) => hir.names.get(&b.name).map(String::as_str),
            HirItem::Fn(f) => hir.names.get(&f.name).map(String::as_str),
            HirItem::Event(e) => hir.names.get(&e.name).map(String::as_str),
            HirItem::Struct(s) => hir.names.get(&s.name).map(String::as_str),
            HirItem::Enum(e) => hir.names.get(&e.name).map(String::as_str),
            _ => None,
        })
        .collect();

    let mut candidates = hir.names.iter().filter(|(_, name)| name.as_str() == word);
    let symbol_id = candidates
        .clone()
        .find(|(_, name)| module_level.contains(name.as_str()))
        .or_else(|| candidates.next())
        .map(|(id, _)| *id)?;

    let typed = typed?;
    let ty = typed.types.get(&symbol_id)?;
    Some(format!("{word}: {}", describe_type(ty)))
}

/// The analyzed document is always [`SourceId`] 0 (the resolver's root, or the
/// lone buffer); other ids are imported modules whose own text maps the range.
fn to_ranged(
    bag: &flux_diagnostics::DiagnosticBag,
    contents: &str,
    sources: &[SourceFile],
) -> Vec<RangedDiagnostic> {
    let locate = |span: Span| -> (&str, Option<String>) {
        if span.source == SourceId(0) {
            return (contents, None);
        }
        sources
            .iter()
            .find(|s| s.id == span.source)
            .map(|s| (s.contents.as_str(), Some(s.path.clone())))
            .unwrap_or((contents, None))
    };
    let mut out = Vec::with_capacity(bag.errors.len() + bag.warnings.len());
    for e in &bag.errors {
        let (text, path) = locate(e.label_span());
        out.push(span_to_ranged(
            e.label_span(),
            e.to_string(),
            e.code().to_string(),
            DiagnosticLevel::Error,
            text,
            path,
        ));
    }
    for w in &bag.warnings {
        let (text, path) = locate(w.label_span());
        out.push(span_to_ranged(
            w.label_span(),
            w.to_string(),
            w.code().to_string(),
            DiagnosticLevel::Warning,
            text,
            path,
        ));
    }
    out
}

fn span_to_ranged(
    span: Span,
    message: String,
    code: String,
    level: DiagnosticLevel,
    contents: &str,
    path: Option<String>,
) -> RangedDiagnostic {
    let (sl, sc, el, ec) = byte_range_to_lsp(contents, span.start, span.end);
    RangedDiagnostic {
        start_line: sl as usize + 1,
        start_column: sc as usize + 1,
        end_line: el as usize + 1,
        end_column: ec as usize + 1,
        message,
        code,
        level,
        path,
    }
}

/// Prefix of the identifier being completed at `line`/`character` (may be empty).
pub fn completion_prefix(contents: &str, line: u32, character: u32) -> String {
    let offset = position_to_offset(contents, line, character);
    let before = &contents[..offset.min(contents.len())];
    let start = before
        .char_indices()
        .rev()
        .find(|(_, c)| !is_ident(*c))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    before[start..].to_string()
}

/// Module-level outline entry for document symbols.
pub struct OutlineSymbol {
    pub name: String,
    pub span: Span,
    pub kind: OutlineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    Variable,
    Function,
    Event,
    Struct,
    Enum,
    Trait,
}

pub fn outline_symbols(hir: &HirProgram) -> Vec<OutlineSymbol> {
    let mut out = Vec::new();
    for item in &hir.items {
        let entry = match item {
            HirItem::Binding(b) => hir.names.get(&b.name).map(|name| OutlineSymbol {
                name: name.clone(),
                span: b.span,
                kind: OutlineKind::Variable,
            }),
            HirItem::Fn(f) => hir.names.get(&f.name).map(|name| OutlineSymbol {
                name: name.clone(),
                span: f.span,
                kind: OutlineKind::Function,
            }),
            HirItem::Event(e) => hir.names.get(&e.name).map(|name| OutlineSymbol {
                name: name.clone(),
                span: e.span,
                kind: OutlineKind::Event,
            }),
            HirItem::Struct(s) => hir.names.get(&s.name).map(|name| OutlineSymbol {
                name: name.clone(),
                span: s.span,
                kind: OutlineKind::Struct,
            }),
            HirItem::Enum(e) => hir.names.get(&e.name).map(|name| OutlineSymbol {
                name: name.clone(),
                span: e.span,
                kind: OutlineKind::Enum,
            }),
            HirItem::Trait(t) => hir.names.get(&t.name).map(|name| OutlineSymbol {
                name: name.clone(),
                span: t.span,
                kind: OutlineKind::Trait,
            }),
            _ => None,
        };
        if let Some(sym) = entry {
            if !sym.name.starts_with("__") {
                out.push(sym);
            }
        }
    }
    out
}

/// Resolve the symbol under the cursor using the reference index.
pub fn symbol_at_position(
    analysis: &DocAnalysis,
    contents: &str,
    line: u32,
    character: u32,
) -> Option<SymbolId> {
    let hir = analysis.hir.as_ref()?;
    let index = analysis.references.as_ref()?;
    let offset = position_to_offset(contents, line, character);
    let source_id = buffer_source_id(analysis)?;
    symbol_at_offset(hir, index, source_id, offset)
}

/// Build rename edit ranges per file path for `symbol_id`.
pub fn rename_edits_for_symbol(
    analysis: &DocAnalysis,
    buffer_contents: &str,
    buffer_path: &str,
    symbol_id: SymbolId,
) -> HashMap<String, Vec<(u32, u32, u32, u32)>> {
    let Some(hir) = analysis.hir.as_ref() else {
        return HashMap::new();
    };
    let Some(index) = analysis.references.as_ref() else {
        return HashMap::new();
    };

    let sources = analysis_sources(analysis, buffer_contents, buffer_path);
    let spans = rename_spans(hir, index, symbol_id, &sources);
    let mut by_path: HashMap<String, Vec<(u32, u32, u32, u32)>> = HashMap::new();

    for span in spans {
        let path = analysis
            .source_paths
            .get(&span.source)
            .cloned()
            .unwrap_or_else(|| buffer_path.to_string());
        let text = sources
            .get(&span.source)
            .map(String::as_str)
            .unwrap_or(buffer_contents);
        by_path
            .entry(normalize_path(&path, buffer_path))
            .or_default()
            .push(byte_range_to_lsp(text, span.start, span.end));
    }

    by_path
}

fn buffer_source_id(analysis: &DocAnalysis) -> Option<SourceId> {
    if analysis.source_paths.is_empty() {
        return Some(SourceId(0));
    }
    if analysis.source_paths.len() == 1 {
        return analysis.source_paths.keys().next().copied();
    }
    analysis
        .source_paths
        .iter()
        .find(|(_, p)| p.as_str() == "<buffer>")
        .map(|(id, _)| *id)
        .or_else(|| analysis.source_paths.keys().next().copied())
}

fn analysis_sources(
    analysis: &DocAnalysis,
    buffer_contents: &str,
    buffer_path: &str,
) -> HashMap<SourceId, String> {
    let mut map = HashMap::new();
    if analysis.source_paths.is_empty() {
        map.insert(SourceId(0), buffer_contents.to_string());
        return map;
    }
    for (id, path) in &analysis.source_paths {
        let text = if path == "<buffer>" || paths_equal(path, buffer_path) {
            buffer_contents.to_string()
        } else {
            std::fs::read_to_string(path).unwrap_or_default()
        };
        map.insert(*id, text);
    }
    map
}

fn paths_equal(a: &str, b: &str) -> bool {
    std::path::Path::new(a) == std::path::Path::new(b)
}

fn normalize_path(path: &str, buffer_path: &str) -> String {
    if path == "<buffer>" {
        buffer_path.to_string()
    } else {
        path.to_string()
    }
}

/// Legacy text-based rename helper (superseded by [`rename_edits_for_symbol`]).
#[allow(dead_code)]
pub fn rename_occurrences(contents: &str, old_name: &str) -> Vec<(u32, u32, u32, u32)> {
    let mut edits = Vec::new();
    for (line_idx, line) in contents.lines().enumerate() {
        let mut col = 0usize;
        let chars: Vec<char> = line.chars().collect();
        while col < chars.len() {
            let start = col;
            while col < chars.len() && is_ident(chars[col]) {
                col += 1;
            }
            if col > start {
                let word: String = chars[start..col].iter().collect();
                if word == old_name {
                    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
                    let byte_end: usize = chars[..col].iter().map(|c| c.len_utf8()).sum();
                    edits.push((
                        line_idx as u32,
                        byte_start as u32,
                        line_idx as u32,
                        byte_end as u32,
                    ));
                }
            } else {
                col += 1;
            }
        }
    }
    edits
}

/// Hover range for a symbol name in `contents`.
pub fn hover_range_for_symbol(
    analysis: &DocAnalysis,
    contents: &str,
    word: &str,
) -> Option<(u32, u32, u32, u32)> {
    let loc = analysis.symbols.by_name.get(word)?;
    span_to_lsp_range(contents, &loc.span, &analysis.source_paths)
}

/// Extracts the identifier word touching or containing `offset` in `contents`.
pub fn word_at_offset(contents: &str, offset: usize) -> Option<&str> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let bytes = contents.as_bytes();
    if offset > bytes.len() {
        return None;
    }
    let mut start = offset;
    while start > 0 && contents[..start].chars().next_back().is_some_and(is_ident) {
        start -= contents[..start].chars().next_back().unwrap().len_utf8();
    }
    let mut end = offset;
    while end < contents.len() && contents[end..].chars().next().is_some_and(is_ident) {
        end += contents[end..].chars().next().unwrap().len_utf8();
    }
    if start == end {
        None
    } else {
        Some(&contents[start..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_edits_respect_shadowing() {
        let src = "fn f() -> Int {\n  let x = 2\n  let y = x\n  return y\n}\n";
        let analysis = analyze_document(None, src);
        assert!(analysis.typed.is_some(), "{:?}", analysis.diagnostics);
        let sym = symbol_at_position(&analysis, src, 2, 10).expect("inner x");
        let edits = rename_edits_for_symbol(&analysis, src, "<buffer>", sym);
        let buffer_edits = edits.get("<buffer>").expect("buffer edits");
        assert_eq!(buffer_edits.len(), 2, "inner let + use");
    }

    #[test]
    fn symbol_index_finds_binding() {
        let src = "let threshold = 3\n";
        let analysis = analyze_document(None, src);
        let loc = analysis
            .symbols
            .by_name
            .get("threshold")
            .expect("threshold");
        assert_eq!(loc.path, "<buffer>");
    }

    #[test]
    fn member_completions_for_struct_field() {
        let src =
            "struct Point { x: Int, y: Int }\nlet p = Point { x: 1, y: 2 }\nlet scale = p.x\n";
        let analysis = analyze_document(None, src);
        assert!(analysis.typed.is_some(), "{:?}", analysis.diagnostics);
        let offset = position_to_offset(src, 2, 13);
        assert!(dot_member_access(src, offset));
        let items = member_completions(&analysis, src, 2, 13);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"x"), "{labels:?}");
        assert!(labels.contains(&"y"), "{labels:?}");
    }

    #[test]
    fn member_completions_for_trait_method() {
        let src = "trait Display { fn to_string(x: Int) -> String; }\nimpl Display for Int { fn to_string(self: Int) -> String { \"i\" } }\nlet msg = (42).to_string()\n";
        let analysis = analyze_document(None, src);
        assert!(analysis.typed.is_some(), "{:?}", analysis.diagnostics);
        let items = member_completions(&analysis, src, 2, 14);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"to_string"), "{labels:?}");
    }
}
