//! Detect direct cycles before type inference needs the binding types.

use std::collections::{HashMap, HashSet};

use flux_hir::{BindingKind, HirExpr, HirItem, HirProgram, SymbolId};

pub(super) fn find_derived_cycle(program: &HirProgram) -> Option<Vec<SymbolId>> {
    let derived: HashSet<_> = program
        .items
        .iter()
        .filter_map(|item| match item {
            HirItem::Binding(binding) if binding.kind == BindingKind::Derived => Some(binding.name),
            _ => None,
        })
        .collect();
    let mut edges = HashMap::<SymbolId, Vec<SymbolId>>::new();
    for item in &program.items {
        let HirItem::Binding(binding) = item else {
            continue;
        };
        if !derived.contains(&binding.name) {
            continue;
        }
        let mut refs = HashSet::new();
        collect_refs(&binding.value, &mut refs);
        edges.insert(
            binding.name,
            refs.into_iter().filter(|id| derived.contains(id)).collect(),
        );
    }
    let mut completed = HashSet::new();
    let mut path = Vec::new();
    for &start in &derived {
        if let Some(cycle) = visit(start, &edges, &mut completed, &mut path) {
            return Some(cycle);
        }
    }
    None
}

fn visit(
    node: SymbolId,
    edges: &HashMap<SymbolId, Vec<SymbolId>>,
    completed: &mut HashSet<SymbolId>,
    path: &mut Vec<SymbolId>,
) -> Option<Vec<SymbolId>> {
    if let Some(index) = path.iter().position(|id| *id == node) {
        let mut cycle = path[index..].to_vec();
        cycle.push(node);
        return Some(cycle);
    }
    if completed.contains(&node) {
        return None;
    }
    path.push(node);
    for &next in edges.get(&node).into_iter().flatten() {
        if let Some(cycle) = visit(next, edges, completed, path) {
            return Some(cycle);
        }
    }
    path.pop();
    completed.insert(node);
    None
}

fn collect_refs(expr: &HirExpr, out: &mut HashSet<SymbolId>) {
    match expr {
        HirExpr::Ident { symbol, .. } => {
            out.insert(*symbol);
        }
        HirExpr::Snap { expr, .. } | HirExpr::Unary { expr, .. } | HirExpr::Cast { expr, .. } => {
            collect_refs(expr, out)
        }
        HirExpr::Call { callee, args, .. } => {
            collect_refs(callee, out);
            for arg in args {
                collect_refs(arg, out);
            }
        }
        HirExpr::Field { base, .. } => collect_refs(base, out),
        HirExpr::Binary { left, right, .. } => {
            collect_refs(left, out);
            collect_refs(right, out);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_refs(cond, out);
            collect_refs(then_expr, out);
            collect_refs(else_expr, out);
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                collect_refs(elem, out);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, value) in fields {
                collect_refs(value, out);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(value) = payload {
                collect_refs(value, out);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            collect_refs(scrutinee, out);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_refs(guard, out);
                }
                collect_refs(&arm.expr, out);
            }
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. }
        | HirExpr::Closure { .. } => {}
    }
}
