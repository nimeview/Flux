//! Symbol reference index for IDE rename and find-references.

use std::collections::HashMap;

use flux_diagnostics::{SourceId, Span};

use crate::nodes::*;
use crate::symbol::SymbolId;

#[derive(Debug, Clone, Default)]
pub struct ReferenceIndex {
    pub references: HashMap<SymbolId, Vec<Span>>,
}

/// Collect every identifier use site keyed by resolved [`SymbolId`].
pub fn build_reference_index(hir: &HirProgram) -> ReferenceIndex {
    let mut references: HashMap<SymbolId, Vec<Span>> = HashMap::new();
    for item in &hir.items {
        collect_refs_item(item, &mut references);
    }
    ReferenceIndex { references }
}

/// Resolve the innermost symbol at a byte offset in `source_id`.
pub fn symbol_at_offset(
    hir: &HirProgram,
    index: &ReferenceIndex,
    source_id: SourceId,
    offset: usize,
) -> Option<SymbolId> {
    let mut best: Option<(usize, SymbolId)> = None;

    let mut consider = |id: SymbolId, span: Span| {
        if span.source != source_id {
            return;
        }
        if offset < span.start || offset >= span.end {
            return;
        }
        let len = span.end.saturating_sub(span.start);
        if best.map(|(best_len, _)| len < best_len).unwrap_or(true) {
            best = Some((len, id));
        }
    };

    for (id, spans) in &index.references {
        for span in spans {
            consider(*id, *span);
        }
    }
    for (id, sym) in &hir.symbols {
        consider(*id, sym.span);
    }

    best.map(|(_, id)| id)
}

/// All spans to rewrite when renaming `symbol_id` (definitions + reads + write targets).
pub fn rename_spans(
    hir: &HirProgram,
    index: &ReferenceIndex,
    symbol_id: SymbolId,
    sources: &HashMap<SourceId, String>,
) -> Vec<Span> {
    let mut out = index
        .references
        .get(&symbol_id)
        .cloned()
        .unwrap_or_default();

    let Some(name) = hir.names.get(&symbol_id) else {
        return dedupe_spans(out);
    };

    if let Some(sym) = hir.symbols.get(&symbol_id) {
        out.push(narrow_name_span(sym.span, name, sources));
    }

    for item in &hir.items {
        collect_write_spans_item(item, symbol_id, name, sources, &mut out);
    }

    dedupe_spans(out)
}

fn push_ref(map: &mut HashMap<SymbolId, Vec<Span>>, id: SymbolId, span: Span) {
    map.entry(id).or_default().push(span);
}

fn collect_refs_item(item: &HirItem, map: &mut HashMap<SymbolId, Vec<Span>>) {
    match item {
        HirItem::Binding(b) => collect_refs_expr(&b.value, map),
        HirItem::Fn(f) => collect_refs_fn(f, map),
        HirItem::Watch(w) => {
            if let Some(t) = w.target {
                push_ref(map, t, w.span);
            }
            collect_refs_block(&w.body, map);
        }
        HirItem::On(o) => {
            push_ref(map, o.event, o.span);
            if let Some(p) = o.param {
                push_ref(map, p, o.span);
            }
            collect_refs_block(&o.body, map);
        }
        HirItem::InitIf(i) => collect_refs_if(i, map),
        HirItem::SourceAssign(a) => {
            push_ref(map, a.name, a.span);
            collect_refs_expr(&a.value, map);
        }
        HirItem::Impl(i) => {
            for m in &i.methods {
                collect_refs_fn(m, map);
            }
        }
        HirItem::Struct(_) | HirItem::Enum(_) | HirItem::Trait(_) | HirItem::Event(_) => {}
    }
}

fn collect_refs_fn(f: &HirFn, map: &mut HashMap<SymbolId, Vec<Span>>) {
    for p in &f.params {
        push_ref(map, p.name, p.span);
    }
    collect_refs_block(&f.body, map);
}

fn collect_refs_block(block: &HirBlock, map: &mut HashMap<SymbolId, Vec<Span>>) {
    for stmt in &block.stmts {
        collect_refs_stmt(stmt, map);
    }
}

fn collect_refs_stmt(stmt: &HirStmt, map: &mut HashMap<SymbolId, Vec<Span>>) {
    match stmt {
        HirStmt::Let(l) => collect_refs_expr(&l.value, map),
        HirStmt::Assign(a) => collect_refs_expr(&a.value, map),
        HirStmt::Emit(e) => {
            for arg in &e.args {
                collect_refs_expr(arg, map);
            }
        }
        HirStmt::Return(r) => {
            if let Some(v) = &r.value {
                collect_refs_expr(v, map);
            }
        }
        HirStmt::Expr(e) => collect_refs_expr(e, map),
        HirStmt::If(i) => collect_refs_if(i, map),
        HirStmt::While(w) => {
            collect_refs_expr(&w.cond, map);
            collect_refs_block(&w.body, map);
        }
        HirStmt::For(f) => {
            collect_refs_expr(&f.iter, map);
            collect_refs_block(&f.body, map);
        }
        HirStmt::Watch(w) => {
            if let Some(t) = w.target {
                push_ref(map, t, w.span);
            }
            collect_refs_block(&w.body, map);
        }
        HirStmt::On(o) => {
            push_ref(map, o.event, o.span);
            if let Some(p) = o.param {
                push_ref(map, p, o.span);
            }
            collect_refs_block(&o.body, map);
        }
        HirStmt::Break(_) | HirStmt::Continue(_) => {}
    }
}

fn collect_refs_if(if_: &HirIf, map: &mut HashMap<SymbolId, Vec<Span>>) {
    collect_refs_expr(&if_.cond, map);
    collect_refs_block(&if_.then_branch, map);
    if let Some(el) = &if_.else_branch {
        match el {
            HirElse::If(inner) => collect_refs_if(inner, map),
            HirElse::Block(b) => collect_refs_block(b, map),
        }
    }
}

fn collect_refs_expr(expr: &HirExpr, map: &mut HashMap<SymbolId, Vec<Span>>) {
    match expr {
        HirExpr::Ident { symbol, span } => push_ref(map, *symbol, *span),
        HirExpr::Snap { expr, .. } => collect_refs_expr(expr, map),
        HirExpr::Call { callee, args, .. } => {
            collect_refs_expr(callee, map);
            for a in args {
                collect_refs_expr(a, map);
            }
        }
        HirExpr::Field { base, .. } => collect_refs_expr(base, map),
        HirExpr::Closure { fn_symbol, .. } => push_ref(map, *fn_symbol, expr.span()),
        HirExpr::Unary { expr, .. } => collect_refs_expr(expr, map),
        HirExpr::Binary { left, right, .. } => {
            collect_refs_expr(left, map);
            collect_refs_expr(right, map);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_refs_expr(cond, map);
            collect_refs_expr(then_expr, map);
            collect_refs_expr(else_expr, map);
        }
        HirExpr::Array { elems, .. } => {
            for e in elems {
                collect_refs_expr(e, map);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_refs_expr(v, map);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(p) = payload {
                collect_refs_expr(p, map);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            collect_refs_expr(scrutinee, map);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_refs_expr(g, map);
                }
                collect_refs_block(&arm.body, map);
                collect_refs_expr(&arm.expr, map);
            }
        }
        HirExpr::Cast { expr, .. } => collect_refs_expr(expr, map),
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. } => {}
    }
}

fn collect_write_spans_item(
    item: &HirItem,
    symbol_id: SymbolId,
    name: &str,
    sources: &HashMap<SourceId, String>,
    out: &mut Vec<Span>,
) {
    match item {
        HirItem::Binding(b) if b.name == symbol_id => {
            out.push(narrow_name_span(b.span, name, sources));
        }
        HirItem::Fn(f) => collect_write_spans_block(&f.body, symbol_id, name, sources, out),
        HirItem::Watch(w) => collect_write_spans_block(&w.body, symbol_id, name, sources, out),
        HirItem::On(o) => collect_write_spans_block(&o.body, symbol_id, name, sources, out),
        HirItem::InitIf(i) => collect_write_spans_if(i, symbol_id, name, sources, out),
        HirItem::SourceAssign(a) if a.name == symbol_id => {
            out.push(narrow_name_span(a.span, name, sources));
        }
        HirItem::Impl(i) => {
            for m in &i.methods {
                collect_write_spans_block(&m.body, symbol_id, name, sources, out);
            }
        }
        _ => {}
    }
}

fn collect_write_spans_block(
    block: &HirBlock,
    symbol_id: SymbolId,
    name: &str,
    sources: &HashMap<SourceId, String>,
    out: &mut Vec<Span>,
) {
    for stmt in &block.stmts {
        collect_write_spans_stmt(stmt, symbol_id, name, sources, out);
    }
}

fn collect_write_spans_stmt(
    stmt: &HirStmt,
    symbol_id: SymbolId,
    name: &str,
    sources: &HashMap<SourceId, String>,
    out: &mut Vec<Span>,
) {
    match stmt {
        HirStmt::Assign(a) if a.name == symbol_id => {
            out.push(narrow_name_span(a.span, name, sources));
        }
        HirStmt::Let(l) if l.name == symbol_id => {
            out.push(narrow_name_span(l.span, name, sources));
        }
        HirStmt::For(f) if f.name == symbol_id => {
            out.push(narrow_name_span(f.span, name, sources));
        }
        HirStmt::If(i) => collect_write_spans_if(i, symbol_id, name, sources, out),
        HirStmt::While(w) => collect_write_spans_block(&w.body, symbol_id, name, sources, out),
        HirStmt::Watch(w) => collect_write_spans_block(&w.body, symbol_id, name, sources, out),
        HirStmt::On(o) => collect_write_spans_block(&o.body, symbol_id, name, sources, out),
        _ => {}
    }
}

fn collect_write_spans_if(
    if_: &HirIf,
    symbol_id: SymbolId,
    name: &str,
    sources: &HashMap<SourceId, String>,
    out: &mut Vec<Span>,
) {
    collect_write_spans_block(&if_.then_branch, symbol_id, name, sources, out);
    if let Some(el) = &if_.else_branch {
        match el {
            HirElse::If(inner) => collect_write_spans_if(inner, symbol_id, name, sources, out),
            HirElse::Block(b) => collect_write_spans_block(b, symbol_id, name, sources, out),
        }
    }
}

fn narrow_name_span(span: Span, name: &str, sources: &HashMap<SourceId, String>) -> Span {
    let Some(text) = sources.get(&span.source) else {
        return span;
    };
    if span.end > text.len() {
        return span;
    }
    let slice = &text[span.start..span.end];
    if let Some(rel) = find_whole_word(slice, name) {
        let start = span.start + rel;
        return Span {
            source: span.source,
            start,
            end: start + name.len(),
        };
    }
    span
}

fn find_whole_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut start = 0usize;
    while start <= haystack.len().saturating_sub(needle.len()) {
        if let Some(pos) = haystack[start..].find(needle) {
            let abs = start + pos;
            let before_ok =
                abs == 0 || !is_ident_byte(haystack.as_bytes().get(abs.wrapping_sub(1)).copied());
            let after = abs + needle.len();
            let after_ok =
                after >= haystack.len() || !is_ident_byte(haystack.as_bytes().get(after).copied());
            if before_ok && after_ok {
                return Some(abs);
            }
            start = abs + 1;
        } else {
            break;
        }
    }
    None
}

fn is_ident_byte(b: Option<u8>) -> bool {
    b.map(|c| c.is_ascii_alphanumeric() || c == b'_')
        .unwrap_or(false)
}

fn dedupe_spans(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by_key(|s| (s.source.0, s.start, s.end));
    spans.dedup_by(|a, b| a.source == b.source && a.start == b.start && a.end == b.end);
    spans
}

/// Extract the identifier touching `offset` in `text`.
pub fn word_at_offset(text: &str, offset: usize) -> Option<&str> {
    if offset > text.len() {
        return None;
    }
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut start = offset;
    while start > 0 && text[..start].chars().next_back().is_some_and(is_ident) {
        start -= text[..start].chars().next_back().unwrap().len_utf8();
    }
    let mut end = offset;
    while end < text.len() && text[end..].chars().next().is_some_and(is_ident) {
        end += text[end..].chars().next().unwrap().len_utf8();
    }
    if start == end {
        None
    } else {
        Some(&text[start..end])
    }
}

#[cfg(test)]
mod tests {
    use flux_diagnostics::{DiagnosticBag, SourceId};
    use flux_syntax::parse_program;

    use crate::lower_program;

    use super::*;

    #[test]
    fn symbol_at_offset_respects_shadowing() {
        let src = "let x = 1\nfn f() {\n  let x = 2\n  let y = x\n}\n";
        let mut diags = DiagnosticBag::new();
        let sid = SourceId(0);
        let ast = parse_program(sid, src, &mut diags).expect("parse");
        let hir = lower_program(&ast, &mut diags).expect("lower");
        let index = build_reference_index(&hir);
        let sources = HashMap::from([(sid, src.to_string())]);

        let inner_use = src.find("let y = x").unwrap() + "let y = ".len();
        let inner_id = symbol_at_offset(&hir, &index, sid, inner_use).expect("inner x");
        let inner_name = hir.names.get(&inner_id).expect("name");
        assert_eq!(inner_name, "x");

        let spans = rename_spans(&hir, &index, inner_id, &sources);
        for span in &spans {
            let slice = &src[span.start..span.end];
            assert_eq!(slice, "x");
        }
        assert!(spans.len() >= 2, "inner binding + use");
    }
}
