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
use flux_hir::{BindingKind, HirFn, HirItem, HirProgram, SymbolId};

use crate::{CompiledProgram, IrNode, IrStmt, NodeId, NodeKind};

use codegen::lower_expr;
use graph::{find_dependency_cycle, format_cycle, push_on_effect_node, push_watch_effect_node};
use items::{const_expr, lower_fn, lower_if_stmt, lower_on, lower_watch};
use reads::collect_reads_expr;

pub fn lower_to_ir(
    program: &HirProgram,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) -> Option<CompiledProgram> {
    let mut nodes = Vec::new();
    let mut init_stmts = Vec::new();
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
                    init_stmts.push(IrStmt::Assign {
                        target: target.clone(),
                        expr: lower_expr(&b.value, program),
                    });
                }
            }
            HirItem::Binding(_) | HirItem::Event(_) => {}
            HirItem::SourceAssign(a) => {
                if let Some(target) = program.names.get(&a.name) {
                    init_stmts.push(IrStmt::Assign {
                        target: target.clone(),
                        expr: lower_expr(&a.value, program),
                    });
                }
            }
            HirItem::InitIf(i) => {
                init_stmts.push(lower_if_stmt(i, program));
            }
            HirItem::Fn(f) => {
                if let Some(name) = program.names.get(&f.name) {
                    functions.push(lower_fn(f, name, program));
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
            source_id: source,
        });
        return None;
    }

    Some(CompiledProgram {
        nodes,
        init_stmts,
        functions,
        watches,
        on_handlers,
        events,
    })
}
