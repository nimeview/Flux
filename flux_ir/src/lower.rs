//! HIR → IR lowering driver.
//!
//! Builds the reactive dependency graph (source/derived/event/effect nodes),
//! compiles function/effect bodies to stack-machine bytecode and validates the
//! graph is acyclic. Split into submodules:
//! - [`graph`]   — effect nodes, dep resolution, cycle detection
//! - [`items`]   — function / effect / statement / block lowering
//! - [`reads`]   — static reactive-dependency collection
//! - [`codegen`] — expression → bytecode emission

mod codegen;
mod graph;
mod items;
mod reads;

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId, Span};
use flux_hir::{BindingKind, HirFn, HirItem, HirProgram, SymbolId, SymbolKind};

use crate::{CompiledProgram, IrNode, IrStmt, NodeId, NodeKind, SourceMap};

use codegen::lower_expr;
use graph::{find_dependency_cycle, format_cycle, push_on_effect_node, push_watch_effect_node};
use items::{const_expr, lower_fn, lower_if_stmt, lower_on, lower_watch, source_loc};
use reads::collect_reads_expr;

/// Runtime key for a HIR symbol. Locals and parameters have unique slots so
/// lexical shadowing cannot alias two different symbols with the same spelling.
/// Module-level names remain stable because the graph and function table use them.
pub(super) fn runtime_symbol_name(program: &HirProgram, id: &SymbolId) -> Option<String> {
    let name = program.names.get(id)?;
    match program.symbols.get(id).map(|symbol| symbol.kind) {
        Some(SymbolKind::Local { .. } | SymbolKind::Param) => {
            Some(format!("\0flux_local_{}", id.0))
        }
        Some(SymbolKind::Function { .. }) if flux_std::is_builtin_fn(name) => {
            Some(format!("\0flux_user_fn_{}", id.0))
        }
        _ => Some(name.clone()),
    }
}

pub fn lower_to_ir(
    program: &HirProgram,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<CompiledProgram> {
    let mut nodes = Vec::new();
    // (statement, target binding, value read set or None for a barrier)
    let mut init_items: Vec<InitItem> = Vec::new();
    let mut functions = Vec::new();
    let mut watches = Vec::new();
    let mut on_handlers = Vec::new();
    let mut events = Vec::new();
    let mut symbol_to_node: HashMap<SymbolId, NodeId> = HashMap::new();
    let mut binding_spans: HashMap<SymbolId, Span> = HashMap::new();

    let reactive_fns: HashMap<String, &HirFn> = program
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Fn(f) if f.reactive => {
                program.names.get(&f.name).map(|name| (name.clone(), f))
            }
            _ => None,
        })
        .collect();

    // Every function, for initialization order: an initializer depends on the
    // bindings it reads directly or through any function it calls.
    let all_fns: HashMap<String, &HirFn> = program
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Fn(f) => program.names.get(&f.name).map(|name| (name.clone(), f)),
            _ => None,
        })
        .collect();

    for item in &program.items {
        let HirItem::Binding(b) = item else { continue };
        binding_spans.insert(b.name, b.span);
        let name = program
            .names
            .get(&b.name)
            .cloned()
            .unwrap_or_else(|| "?".into());
        let id = NodeId(nodes.len() as u32);
        symbol_to_node.insert(b.name, id);
        nodes.push(match b.kind {
            BindingKind::Source => IrNode {
                name,
                kind: NodeKind::Source,
                deps: vec![],
                init: const_expr(&b.value),
                code: vec![],
                effect: None,
            },
            BindingKind::Derived => IrNode {
                name,
                kind: NodeKind::Derived,
                deps: vec![],
                init: None,
                code: vec![],
                effect: None,
            },
        });
    }

    for item in &program.items {
        let HirItem::Binding(b) = item else { continue };
        if b.kind != BindingKind::Derived {
            continue;
        }
        let id = symbol_to_node[&b.name];
        let reads = collect_reads_expr(&b.value, program, &reactive_fns);
        let deps: Vec<NodeId> = reads
            .iter()
            .filter_map(|sym| symbol_to_node.get(sym).copied())
            .collect();
        let node = &mut nodes[id.0 as usize];
        node.deps = deps;
        node.code = lower_expr(&b.value, program);
    }

    // Event nodes (graph vertices for `emit` / `on` wiring).
    for item in &program.items {
        let HirItem::Event(ev) = item else { continue };
        let name = program
            .names
            .get(&ev.name)
            .cloned()
            .unwrap_or_else(|| "?".into());
        let id = NodeId(nodes.len() as u32);
        symbol_to_node.insert(ev.name, id);
        events.push(name.clone());
        nodes.push(IrNode {
            name,
            kind: NodeKind::Event,
            deps: vec![],
            init: None,
            code: vec![],
            effect: None,
        });
    }

    for item in &program.items {
        match item {
            // A source whose initializer is not a literal (`let mut a = -x + 1`)
            // starts as Unit and is assigned during module init, in declaration order.
            HirItem::Binding(b)
                if b.kind == BindingKind::Source && const_expr(&b.value).is_none() =>
            {
                if let Some(target) = program.names.get(&b.name) {
                    init_items.push(InitItem {
                        stmt: IrStmt::Assign {
                            target: target.clone(),
                            slot: None,
                            expr: lower_expr(&b.value, program),
                        },
                        target: Some(b.name),
                        reads: Some(collect_reads_expr(&b.value, program, &all_fns)),
                    });
                }
            }
            HirItem::Binding(_) | HirItem::Event(_) => {}
            HirItem::SourceAssign(a) => {
                if let Some(target) = program.names.get(&a.name) {
                    init_items.push(InitItem {
                        stmt: IrStmt::Assign {
                            target: target.clone(),
                            slot: None,
                            expr: lower_expr(&a.value, program),
                        },
                        target: None,
                        reads: None,
                    });
                }
            }
            HirItem::InitIf(i) => {
                init_items.push(InitItem {
                    stmt: lower_if_stmt(i, program),
                    target: None,
                    reads: None,
                });
            }
            HirItem::Fn(f) => {
                if let Some(name) = runtime_symbol_name(program, &f.name) {
                    functions.push(lower_fn(f, &name, program));
                }
            }
            HirItem::Watch(w) => {
                let watch_idx = watches.len();
                let compiled = lower_watch(w, program, &reactive_fns);
                watches.push(compiled);
                push_watch_effect_node(
                    &mut nodes,
                    watch_idx,
                    w,
                    &watches[watch_idx],
                    program,
                    &symbol_to_node,
                );
            }
            HirItem::On(o) => {
                let on_idx = on_handlers.len();
                on_handlers.push(lower_on(o, program));
                push_on_effect_node(&mut nodes, on_idx, o, program, &symbol_to_node);
            }
            HirItem::Struct(_) | HirItem::Enum(_) | HirItem::Trait(_) | HirItem::Impl(_) => {}
        }
    }

    let init_stmts = order_init_stmts(init_items, program, &binding_spans, diagnostics)?;

    if let Some(cycle) = find_dependency_cycle(&nodes) {
        let cycle_names: Vec<String> = cycle.iter().map(|&idx| nodes[idx].name.clone()).collect();
        let cycle_text = format_cycle(&cycle_names);
        let name_to_symbol: HashMap<&str, SymbolId> = program
            .names
            .iter()
            .map(|(id, name)| (name.as_str(), *id))
            .collect();
        let span = cycle
            .iter()
            .find_map(|&idx| {
                name_to_symbol
                    .get(nodes[idx].name.as_str())
                    .and_then(|sym| binding_spans.get(sym).copied())
            })
            .unwrap_or(Span {
                source,
                start: 0,
                end: 0,
            });

        diagnostics.push(FluxError::CyclicDependency {
            cycle: cycle_text,
            span: span_to_miette(span),
            source_id: span.source,
        });
        return None;
    }

    let bindings = binding_spans
        .iter()
        .filter_map(|(id, span)| Some((runtime_symbol_name(program, id)?, source_loc(*span))))
        .collect();
    let mut program = CompiledProgram {
        source_map: SourceMap {
            files: Vec::new(),
            bindings,
        },
        nodes,
        init_stmts,
        functions,
        watches,
        on_handlers,
        events,
    };
    crate::resolve_locals(&mut program);
    Some(program)
}

struct InitItem {
    stmt: IrStmt,
    /// The source binding this statement initializes, if it is one.
    target: Option<SymbolId>,
    /// Bindings its value reads (through called functions too); `None` marks
    /// a barrier (a top-level `if` or assignment) that keeps its position.
    reads: Option<std::collections::HashSet<SymbolId>>,
}

/// Orders module initialization: a computed source initializer runs after
/// the initializers of the bindings it reads (declaration order otherwise),
/// so `let a = f()` may use a `const` declared later. Barriers stay in place
/// relative to everything before and after them. A cycle is an error.
fn order_init_stmts(
    items: Vec<InitItem>,
    program: &HirProgram,
    binding_spans: &HashMap<SymbolId, Span>,
    diagnostics: &mut DiagnosticBag,
) -> Option<Vec<IrStmt>> {
    let target_index: HashMap<SymbolId, usize> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| item.target.map(|t| (t, i)))
        .collect();
    let n = items.len();
    let mut deps: Vec<std::collections::BTreeSet<usize>> = vec![Default::default(); n];
    let mut last_barrier: Option<usize> = None;
    for (i, item) in items.iter().enumerate() {
        match &item.reads {
            None => {
                deps[i].extend(0..i);
                last_barrier = Some(i);
            }
            Some(reads) => {
                deps[i].extend(last_barrier);
                deps[i].extend(
                    reads
                        .iter()
                        .filter_map(|sym| target_index.get(sym).copied())
                        .filter(|&j| j != i),
                );
            }
        }
    }
    // Statements after a barrier stay after it.
    for (i, item) in items.iter().enumerate() {
        if item.reads.is_none() {
            for later in deps.iter_mut().skip(i + 1) {
                later.insert(i);
            }
        }
    }
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let Some(next) = (0..n).find(|&i| !done[i] && deps[i].iter().all(|&d| done[d])) else {
            let stuck: Vec<String> = (0..n)
                .filter(|&i| !done[i])
                .filter_map(|i| items[i].target)
                .filter_map(|t| program.names.get(&t).cloned())
                .collect();
            let span = (0..n)
                .filter(|&i| !done[i])
                .find_map(|i| items[i].target.and_then(|t| binding_spans.get(&t).copied()))
                .unwrap_or(Span::DUMMY);
            diagnostics.push(FluxError::CyclicDependency {
                cycle: format_cycle(&stuck),
                span: span_to_miette(span),
                source_id: span.source,
            });
            return None;
        };
        done[next] = true;
        order.push(next);
    }
    let mut slots: Vec<Option<IrStmt>> = items.into_iter().map(|item| Some(item.stmt)).collect();
    Some(order.into_iter().filter_map(|i| slots[i].take()).collect())
}
