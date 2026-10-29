use std::collections::{HashMap, VecDeque};

use flux_ir::EffectMeta;
use flux_value::Value;

pub type RuntimeValue = Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone)]
pub enum NodeKind {
    Source,
    Derived {
        deps: Vec<NodeId>,
    },
    /// Declared event — subscribers are [`NodeKind::Effect`] nodes in `dependents`.
    Event,
    /// `watch` / `on` handler wired into the graph.
    Effect {
        meta: EffectMeta,
    },
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub value: RuntimeValue,
}

#[derive(Debug, Default)]
pub struct ReactiveGraph {
    pub nodes: Vec<Node>,
    pub name_to_id: HashMap<String, NodeId>,
    pub topo_order: Vec<NodeId>,
    pub dependents: Vec<Vec<NodeId>>,
}

impl ReactiveGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_source(&mut self, name: &str, value: RuntimeValue) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.name_to_id.insert(name.to_string(), id);
        self.nodes.push(Node {
            name: name.to_string(),
            kind: NodeKind::Source,
            value,
        });
        self.dependents.push(Vec::new());
        self.topo_order.push(id);
        id
    }

    pub fn add_derived(&mut self, name: &str, deps: Vec<NodeId>) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.name_to_id.insert(name.to_string(), id);
        self.nodes.push(Node {
            name: name.to_string(),
            kind: NodeKind::Derived { deps: deps.clone() },
            value: RuntimeValue::Unit,
        });
        self.dependents.push(Vec::new());
        for dep in deps {
            self.dependents[dep.0 as usize].push(id);
        }
        self.rebuild_topo();
        id
    }

    pub fn add_event(&mut self, name: &str) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.name_to_id.insert(name.to_string(), id);
        self.nodes.push(Node {
            name: name.to_string(),
            kind: NodeKind::Event,
            value: RuntimeValue::Unit,
        });
        self.dependents.push(Vec::new());
        self.topo_order.push(id);
        id
    }

    pub fn add_effect(&mut self, name: &str, deps: Vec<NodeId>, meta: EffectMeta) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.name_to_id.insert(name.to_string(), id);
        self.nodes.push(Node {
            name: name.to_string(),
            kind: NodeKind::Effect { meta },
            value: RuntimeValue::Unit,
        });
        self.dependents.push(Vec::new());
        for dep in deps {
            self.dependents[dep.0 as usize].push(id);
        }
        self.rebuild_topo();
        id
    }

    /// Effect nodes subscribed to an event (via `dependents` of the event node).
    pub fn effects_for_event(&self, event_id: NodeId) -> &[NodeId] {
        &self.dependents[event_id.0 as usize]
    }

    pub fn get(&self, id: NodeId) -> &RuntimeValue {
        &self.nodes[id.0 as usize].value
    }

    pub fn get_by_name(&self, name: &str) -> Option<&RuntimeValue> {
        self.name_to_id.get(name).map(|id| self.get(*id))
    }

    pub fn set_source(&mut self, id: NodeId, value: RuntimeValue) {
        self.nodes[id.0 as usize].value = value;
        self.propagate_from(id);
    }

    pub fn set_by_name(&mut self, name: &str, value: RuntimeValue) -> bool {
        let Some(id) = self.name_to_id.get(name).copied() else {
            return false;
        };
        if !matches!(self.nodes[id.0 as usize].kind, NodeKind::Source) {
            return false;
        }
        self.set_source(id, value);
        true
    }

    /// Replace a derived node's dependency edges (runtime dynamic tracking).
    pub fn set_derived_deps(&mut self, id: NodeId, new_deps: Vec<NodeId>) {
        let NodeKind::Derived { deps: old_deps } = self.nodes[id.0 as usize].kind.clone() else {
            return;
        };
        if old_deps == new_deps {
            return;
        }

        use std::collections::HashSet;
        let old_set: HashSet<NodeId> = old_deps.iter().copied().collect();
        let new_set: HashSet<NodeId> = new_deps.iter().copied().collect();

        if old_set == new_set {
            self.nodes[id.0 as usize].kind = NodeKind::Derived { deps: new_deps };
            return;
        }

        for old in old_deps.iter().filter(|d| !new_set.contains(d)) {
            self.dependents[old.0 as usize].retain(|&d| d != id);
        }
        for new in new_deps.iter().filter(|d| !old_set.contains(d)) {
            self.dependents[new.0 as usize].push(id);
        }

        self.nodes[id.0 as usize].kind = NodeKind::Derived { deps: new_deps };
        self.rebuild_topo();
    }

    fn rebuild_topo(&mut self) {
        let n = self.nodes.len();
        let mut indegree = vec![0usize; n];
        for list in &self.dependents {
            for &d in list {
                indegree[d.0 as usize] += 1;
            }
        }

        let mut queue = VecDeque::new();
        for (i, &deg) in indegree.iter().enumerate().take(n) {
            if deg == 0 {
                queue.push_back(NodeId(i as u32));
            }
        }

        let mut order = Vec::with_capacity(n);
        while let Some(id) = queue.pop_front() {
            order.push(id);
            for &dep in &self.dependents[id.0 as usize] {
                let d = &mut indegree[dep.0 as usize];
                *d -= 1;
                if *d == 0 {
                    queue.push_back(dep);
                }
            }
        }

        self.topo_order = order;
    }

    /// Returns `false` when dependency edges contain a cycle.
    pub fn is_acyclic(&self) -> bool {
        self.topo_order.len() == self.nodes.len()
    }

    fn propagate_from(&mut self, _changed: NodeId) {
        // Recomputation is handled by Executor.
    }
}
