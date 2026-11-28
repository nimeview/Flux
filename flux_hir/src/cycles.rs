//! E029: watches that wake each other in a cycle.
//!
//! A watch wakes another when it changes a source the other follows: through
//! its own assignments, the functions, methods, closures, and function values
//! it calls, and the handlers of the events it emits, at any depth. A watch
//! follows its target, or (without one) what its body names, and what the
//! derived values and tracked functions among those read, as the runtime does;
//! what an `act fn` or a handler reads is not followed. Each watch runs at most
//! once per batch, so a cycle ends, but its last change does not reach the
//! watch it would wake.

use std::collections::{BTreeSet, HashMap, HashSet};

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxWarning, Span};

use crate::nodes::*;
use crate::{BindingKind, SymbolId};

/// What a body does, directly.
#[derive(Default, Clone)]
struct Effects {
    reads: HashSet<SymbolId>,
    writes: HashSet<SymbolId>,
    emits: HashSet<SymbolId>,
    /// Methods called by name (`x.bump()`), resolved by [`resolve_methods`].
    methods: HashSet<String>,
}

pub fn check_watch_cycles(hir: &HirProgram, diagnostics: &mut DiagnosticBag) {
    let watches: Vec<&HirWatch> = hir
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Watch(w) => Some(w),
            _ => None,
        })
        .collect();
    if watches.is_empty() {
        return;
    }
    let sources: HashSet<SymbolId> = hir
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Binding(b) if b.kind == BindingKind::Source => Some(b.name),
            _ => None,
        })
        .collect();

    // A method call may reach any `impl` method with that name.
    let mut method_fns: HashMap<&str, Vec<SymbolId>> = HashMap::new();
    for ((_, method), id) in &hir.trait_impls {
        method_fns.entry(method.as_str()).or_default().push(*id);
    }
    let body_effects = |body: &HirBlock| {
        let mut e = Effects::default();
        block(body, &mut e);
        resolve_methods(&mut e, &method_fns);
        e
    };

    // Functions, events (their handlers), derived values, and sources that
    // hold a function, with what each does once everything it calls or emits
    // is included.
    let mut units: HashMap<SymbolId, Effects> = HashMap::new();
    // What the runtime follows through: derived values and tracked functions.
    let mut direct_reads: HashMap<SymbolId, HashSet<SymbolId>> = HashMap::new();
    for item in &hir.items {
        match item {
            HirItem::Fn(f) => {
                let e = body_effects(&f.body);
                if f.reactive {
                    direct_reads.insert(f.name, e.reads.clone());
                }
                units.insert(f.name, e);
            }
            HirItem::On(o) => {
                let e = body_effects(&o.body);
                let unit = units.entry(o.event).or_default();
                unit.reads.extend(e.reads);
                unit.writes.extend(e.writes);
                unit.emits.extend(e.emits);
            }
            HirItem::Binding(b) => {
                let holds_fn = matches!(b.value, HirExpr::Ident { .. } | HirExpr::Closure { .. });
                if b.kind == BindingKind::Derived || holds_fn {
                    let mut e = Effects::default();
                    expr(&b.value, &mut e);
                    resolve_methods(&mut e, &method_fns);
                    if b.kind == BindingKind::Derived {
                        direct_reads.insert(b.name, e.reads.clone());
                    }
                    units.insert(b.name, e);
                }
            }
            _ => {}
        }
    }
    close(&mut units);

    let effects_of = |body: &HirBlock| {
        let mut e = body_effects(body);
        expand(&mut e, &units);
        e
    };
    let mut writes = Vec::with_capacity(watches.len());
    let mut follows = Vec::with_capacity(watches.len());
    for w in &watches {
        let e = effects_of(&w.body);
        writes.push(
            e.writes
                .iter()
                .copied()
                .filter(|id| sources.contains(id))
                .collect::<HashSet<_>>(),
        );
        let start = match w.target {
            Some(target) => HashSet::from([target]),
            None => body_effects(&w.body).reads,
        };
        follows.push(read_closure(start, &direct_reads));
    }

    // `wakes[i]`: the watches that watch `i` can wake.
    let n = watches.len();
    let wakes: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| writes[i].iter().any(|s| follows[j].contains(s)))
                .collect()
        })
        .collect();
    let reach: Vec<HashSet<usize>> = (0..n).map(|i| reachable(i, &wakes)).collect();
    let mut reported = HashSet::new();
    for i in 0..n {
        if reported.contains(&i) || !reach[i].contains(&i) {
            continue;
        }
        let group: Vec<usize> = (0..n)
            .filter(|&j| reach[i].contains(&j) && reach[j].contains(&i))
            .collect();
        let mut through = BTreeSet::new();
        for &a in &group {
            for &b in &group {
                for s in writes[a].intersection(&follows[b]) {
                    if let Some(name) = hir.names.get(s) {
                        through.insert(name.clone());
                    }
                }
            }
        }
        reported.extend(group.iter().copied());
        let through: Vec<String> = through.into_iter().map(|s| format!("`{s}`")).collect();
        let span = keyword_span(watches[i].span);
        diagnostics.push_warning(FluxWarning::WatchCycle {
            message: if group.len() == 1 {
                format!("this watch wakes itself through {}", through.join(", "))
            } else {
                format!(
                    "this watch and {} other{} wake each other in a cycle through {}",
                    group.len() - 1,
                    if group.len() == 2 { "" } else { "s" },
                    through.join(", ")
                )
            },
            span: span_to_miette(span),
            source_id: span.source,
        });
    }
}

/// Counts each method called by name as a call of every `impl` method with
/// that name.
fn resolve_methods(e: &mut Effects, method_fns: &HashMap<&str, Vec<SymbolId>>) {
    for name in e.methods.drain() {
        if let Some(ids) = method_fns.get(name.as_str()) {
            e.reads.extend(ids);
        }
    }
}

/// The `watch` keyword at the start of a watch.
fn keyword_span(span: Span) -> Span {
    Span {
        end: (span.start + "watch".len()).min(span.end),
        ..span
    }
}

/// `start` and everything the derived values and functions in it read, at
/// any depth.
fn read_closure(
    start: HashSet<SymbolId>,
    direct_reads: &HashMap<SymbolId, HashSet<SymbolId>>,
) -> HashSet<SymbolId> {
    let mut seen = HashSet::new();
    let mut stack: Vec<SymbolId> = start.into_iter().collect();
    while let Some(id) = stack.pop() {
        if seen.insert(id) {
            if let Some(reads) = direct_reads.get(&id) {
                stack.extend(reads);
            }
        }
    }
    seen
}

fn reachable(start: usize, wakes: &[Vec<usize>]) -> HashSet<usize> {
    let mut seen = HashSet::new();
    let mut stack: Vec<usize> = wakes[start].clone();
    while let Some(next) = stack.pop() {
        if seen.insert(next) {
            stack.extend(&wakes[next]);
        }
    }
    seen
}

/// Adds to each unit what the units it names or emits do, until nothing
/// changes.
fn close(units: &mut HashMap<SymbolId, Effects>) {
    // One pass over the strongly connected components, sinks first: a unit
    // does its own effects and everything the units it names do. (A naive
    // fixed point took a round per link of a chain: 9.5 s for 1,000 derived
    // values in a row.)
    let ids: Vec<SymbolId> = units.keys().copied().collect();
    let index: HashMap<SymbolId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let succ: Vec<Vec<usize>> = ids
        .iter()
        .map(|id| {
            let e = &units[id];
            let mut next: Vec<usize> = e
                .reads
                .iter()
                .chain(&e.emits)
                .filter_map(|n| index.get(n).copied())
                .collect();
            next.sort_unstable();
            next.dedup();
            next
        })
        .collect();
    let mut closed: Vec<Option<Effects>> = vec![None; ids.len()];
    for component in components(&succ) {
        let mut e = Effects::default();
        for &u in &component {
            let own = &units[&ids[u]];
            e.reads.extend(&own.reads);
            e.writes.extend(&own.writes);
            e.emits.extend(&own.emits);
        }
        for &u in &component {
            for &v in &succ[u] {
                // Components come sinks first, so `v` is done unless it is in
                // this component.
                if let Some(done) = &closed[v] {
                    e.reads.extend(&done.reads);
                    e.writes.extend(&done.writes);
                    e.emits.extend(&done.emits);
                }
            }
        }
        for &u in &component {
            closed[u] = Some(e.clone());
        }
    }
    for (u, e) in closed.into_iter().enumerate() {
        if let Some(e) = e {
            units.insert(ids[u], e);
        }
    }
}

/// Tarjan's strongly connected components of the graph `succ`, each
/// component after every component it reaches. Iterative: a chain of
/// thousands of values must not exhaust the stack.
fn components(succ: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const NONE: usize = usize::MAX;
    let n = succ.len();
    let mut index = vec![NONE; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let mut next_index = 0;
    for root in 0..n {
        if index[root] != NONE {
            continue;
        }
        // (node, position in its successor list)
        let mut work = vec![(root, 0)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&mut (v, ref mut pos)) = work.last_mut() {
            if let Some(&w) = succ[v].get(*pos) {
                *pos += 1;
                if index[w] == NONE {
                    index[w] = next_index;
                    low[w] = next_index;
                    next_index += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    work.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                low[parent] = low[parent].min(low[v]);
            }
            if low[v] == index[v] {
                let mut component = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                out.push(component);
            }
        }
    }
    out
}

/// Adds what the named functions and derived values, and the emitted events'
/// handlers, do (one level; `close` makes units complete first).
fn expand(e: &mut Effects, units: &HashMap<SymbolId, Effects>) {
    let named: Vec<SymbolId> = e.reads.iter().chain(&e.emits).copied().collect();
    for id in named {
        if let Some(unit) = units.get(&id) {
            e.reads.extend(&unit.reads);
            e.writes.extend(&unit.writes);
            e.emits.extend(&unit.emits);
        }
    }
}

fn block(b: &HirBlock, e: &mut Effects) {
    for s in &b.stmts {
        stmt(s, e);
    }
}

fn stmt(s: &HirStmt, e: &mut Effects) {
    match s {
        HirStmt::Let(l) => expr(&l.value, e),
        HirStmt::Assign(a) => {
            e.writes.insert(a.name);
            // Assigning a part reads the rest of the value.
            if !a.path.is_empty() {
                e.reads.insert(a.name);
            }
            for index in a.indices() {
                expr(index, e);
            }
            expr(&a.value, e);
        }
        HirStmt::Emit(m) => {
            e.emits.insert(m.event);
            for arg in &m.args {
                expr(arg, e);
            }
        }
        HirStmt::Return(r) => {
            if let Some(v) = &r.value {
                expr(v, e);
            }
        }
        HirStmt::Expr(x) => expr(x, e),
        HirStmt::If(i) => if_stmt(i, e),
        HirStmt::While(w) => {
            expr(&w.cond, e);
            block(&w.body, e);
        }
        HirStmt::For(f) => {
            expr(&f.iter, e);
            block(&f.body, e);
        }
        HirStmt::Break(_) | HirStmt::Continue(_) | HirStmt::Watch(_) | HirStmt::On(_) => {}
    }
}

fn if_stmt(i: &HirIf, e: &mut Effects) {
    expr(&i.cond, e);
    block(&i.then_branch, e);
    match &i.else_branch {
        Some(HirElse::If(next)) => if_stmt(next, e),
        Some(HirElse::Block(b)) => block(b, e),
        None => {}
    }
}

fn expr(x: &HirExpr, e: &mut Effects) {
    match x {
        HirExpr::Ident { symbol, .. } => {
            e.reads.insert(*symbol);
        }
        // A closure may run anywhere it is passed: count it as called.
        HirExpr::Closure { fn_symbol, .. } => {
            e.reads.insert(*fn_symbol);
        }
        HirExpr::Snap { expr: inner, .. }
        | HirExpr::Field { base: inner, .. }
        | HirExpr::Unary { expr: inner, .. }
        | HirExpr::Cast { expr: inner, .. }
        | HirExpr::ToText { expr: inner, .. }
        | HirExpr::Try { expr: inner, .. } => expr(inner, e),
        HirExpr::Call { callee, args, .. } => {
            if let HirExpr::Field { field, .. } = callee.as_ref() {
                e.methods.insert(field.clone());
            }
            expr(callee, e);
            for a in args {
                expr(a, e);
            }
        }
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
            expr(left, e);
            expr(right, e);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            expr(cond, e);
            expr(then_expr, e);
            expr(else_expr, e);
        }
        HirExpr::Array { elems, .. } => {
            for el in elems {
                expr(el, e);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                expr(v, e);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(p) = payload {
                expr(p, e);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            expr(scrutinee, e);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    expr(g, e);
                }
                block(&arm.body, e);
                expr(&arm.expr, e);
            }
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. } => {}
    }
}
