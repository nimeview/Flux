//! Dependency-graph utilities: effect-node construction, dep resolution and
//! cycle detection (Kahn topological sort + DFS to recover a concrete cycle).

use std::collections::{HashMap, VecDeque};

use flux_hir::{HirOn, HirProgram, HirWatch, SymbolId};

use crate::{EffectMeta, EffectTrigger, HandlerKind, IrNode, NodeId, NodeKind};

/// Returns node indices forming a cycle, or `None` if the dependency graph is acyclic.
pub(super) fn find_dependency_cycle(nodes: &[IrNode]) -> Option<Vec<usize>> {
    let n = nodes.len();
    if n == 0 {
        return None;
    }

    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indegree = vec![0usize; n];

    for (i, node) in nodes.iter().enumerate() {
        if !is_dataflow_node(node.kind) {
            continue;
        }
        for &dep_id in &node.deps {
            let d = dep_id.0 as usize;
            if d >= n || !is_dataflow_node(nodes[d].kind) {
                continue;
            }
            deps[i].push(d);
            dependents[d].push(i);
            indegree[i] += 1;
        }
    }

    let mut queue = VecDeque::new();
    for (i, &deg) in indegree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut indegree_work = indegree.clone();
    let mut visited = 0usize;
    while let Some(i) = queue.pop_front() {
        visited += 1;
        for &d in &dependents[i] {
            indegree_work[d] -= 1;
            if indegree_work[d] == 0 {
                queue.push_back(d);
            }
        }
    }

    if visited == n {
        return None;
    }

    find_cycle_dfs(n, &deps, &indegree_work)
}

fn is_dataflow_node(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Source | NodeKind::Derived)
}

fn dep_names_to_ids(
    names: &[String],
    program: &HirProgram,
    symbol_to_node: &HashMap<SymbolId, NodeId>,
) -> Vec<NodeId> {
    names
        .iter()
        .filter_map(|name| {
            program
                .names
                .iter()
                .find(|(_, n)| n.as_str() == name.as_str())
                .and_then(|(sym, _)| symbol_to_node.get(sym).copied())
        })
        .collect()
}

pub(super) fn push_watch_effect_node(
    nodes: &mut Vec<IrNode>,
    watch_idx: usize,
    w: &HirWatch,
    compiled: &crate::CompiledWatch,
    program: &HirProgram,
    symbol_to_node: &HashMap<SymbolId, NodeId>,
) {
    let target = w.target.and_then(|t| program.names.get(&t).cloned());
    let deps = if let Some(ref target_name) = target {
        dep_names_to_ids(std::slice::from_ref(target_name), program, symbol_to_node)
    } else {
        dep_names_to_ids(&compiled.deps, program, symbol_to_node)
    };
    nodes.push(IrNode {
        name: format!("__effect_watch_{watch_idx}"),
        kind: NodeKind::Effect,
        deps,
        init: None,
        code: vec![],
        effect: Some(EffectMeta {
            trigger: EffectTrigger::Watch { target },
            handler_index: watch_idx,
            handler_kind: HandlerKind::Watch,
        }),
    });
}

pub(super) fn push_on_effect_node(
    nodes: &mut Vec<IrNode>,
    on_idx: usize,
    o: &HirOn,
    program: &HirProgram,
    symbol_to_node: &HashMap<SymbolId, NodeId>,
) {
    let event_name = program
        .names
        .get(&o.event)
        .cloned()
        .unwrap_or_else(|| "?".into());
    let deps = symbol_to_node.get(&o.event).copied().into_iter().collect();
    nodes.push(IrNode {
        name: format!("__effect_on_{on_idx}"),
        kind: NodeKind::Effect,
        deps,
        init: None,
        code: vec![],
        effect: Some(EffectMeta {
            trigger: EffectTrigger::OnEvent(event_name),
            handler_index: on_idx,
            handler_kind: HandlerKind::On,
        }),
    });
}

fn find_cycle_dfs(
    n: usize,
    deps: &[Vec<usize>],
    remaining_indegree: &[usize],
) -> Option<Vec<usize>> {
    let mut color = vec![0u8; n]; // 0=white, 1=gray, 2=black
    let mut parent = vec![None::<usize>; n];

    for start in 0..n {
        if remaining_indegree[start] == 0 || color[start] != 0 {
            continue;
        }
        if let Some(cycle) = dfs_cycle(start, deps, &mut color, &mut parent) {
            return Some(cycle);
        }
    }
    None
}

fn dfs_cycle(
    u: usize,
    deps: &[Vec<usize>],
    color: &mut [u8],
    parent: &mut [Option<usize>],
) -> Option<Vec<usize>> {
    color[u] = 1;
    for &v in &deps[u] {
        match color[v] {
            0 => {
                parent[v] = Some(u);
                if let Some(cycle) = dfs_cycle(v, deps, color, parent) {
                    return Some(cycle);
                }
            }
            1 => {
                let mut path = vec![v];
                let mut cur = u;
                while cur != v {
                    path.push(cur);
                    cur = parent[cur].unwrap_or(v);
                    if path.len() > deps.len() + 1 {
                        break;
                    }
                }
                path.push(v);
                path.reverse();
                return Some(path);
            }
            _ => {}
        }
    }
    color[u] = 2;
    None
}

pub(super) fn format_cycle(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let mut text = names.join(" → ");
    if names.first() != names.last() {
        if let Some(first) = names.first() {
            text.push_str(" → ");
            text.push_str(first);
        }
    }
    text
}
