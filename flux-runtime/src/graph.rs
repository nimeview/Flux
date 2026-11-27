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
    /// Slots of removed nodes (a dropped instance's), reused by the next ones.
    free: Vec<NodeId>,
    /// Each node's index in `topo_order`. A node moved to the end leaves an
    /// old entry behind, which [`Self::topo`] skips.
    topo_pos: Vec<usize>,
    /// How many such old entries `topo_order` has.
    topo_stale: usize,
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
        self.topo_pos.push(self.topo_order.len());
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
        self.topo_pos.push(self.topo_order.len());
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

    /// Adds a node for an instance: a source with `value`, or a derived value
    /// whose dependencies are set later. Reuses a removed node's slot, which
    /// is still in the topological order (with no edges). Returns the id and
    /// whether the slot is new.
    pub fn add_instance_node(
        &mut self,
        name: String,
        value: Option<RuntimeValue>,
    ) -> (NodeId, bool) {
        let kind = match value {
            Some(_) => NodeKind::Source,
            None => NodeKind::Derived { deps: Vec::new() },
        };
        let node = Node {
            name: name.clone(),
            kind,
            value: value.unwrap_or(RuntimeValue::Unit),
        };
        if let Some(id) = self.free.pop() {
            self.nodes[id.0 as usize] = node;
            self.dependents[id.0 as usize].clear();
            self.name_to_id.insert(name, id);
            // A new node goes after everything that exists.
            self.topo_pos[id.0 as usize] = self.topo_order.len();
            self.topo_order.push(id);
            self.topo_stale += 1;
            if self.topo_stale > self.topo_order.len() / 2 {
                self.rebuild_topo();
            }
            return (id, false);
        }
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        self.dependents.push(Vec::new());
        self.topo_pos.push(self.topo_order.len());
        self.topo_order.push(id);
        self.name_to_id.insert(name, id);
        (id, true)
    }

    /// The nodes in dependency order.
    pub fn topo(&self) -> impl Iterator<Item = NodeId> + '_ {
        let all = self.topo_stale == 0;
        self.topo_order
            .iter()
            .enumerate()
            .filter(move |(i, id)| all || self.topo_pos[id.0 as usize] == *i)
            .map(|(_, id)| *id)
    }

    /// Where `id` is in the dependency order.
    pub fn topo_position(&self, id: NodeId) -> usize {
        self.topo_pos[id.0 as usize]
    }

    /// Whether each of `deps` comes before `id` in the order.
    fn ordered_before(&self, deps: &[NodeId], id: NodeId) -> bool {
        let at = self.topo_pos[id.0 as usize];
        deps.iter().all(|d| self.topo_pos[d.0 as usize] < at)
    }

    /// Sets the dependencies of instance nodes `ids` at once, then orders the
    /// graph once.
    pub fn connect(&mut self, edges: Vec<(NodeId, Vec<NodeId>)>) {
        let mut ordered = true;
        for (id, deps) in edges {
            for &dep in &deps {
                self.dependents[dep.0 as usize].push(id);
            }
            ordered &= self.ordered_before(&deps, id);
            self.nodes[id.0 as usize].kind = NodeKind::Derived { deps };
        }
        if !ordered {
            self.rebuild_topo();
        }
    }

    /// Removes the nodes `ids` (a dropped instance's) and their edges; their
    /// slots stay, empty, for reuse. Their values are returned, so that the
    /// caller drops them (and the instances they hold) when it is safe.
    pub fn remove_nodes(&mut self, ids: &[NodeId]) -> Vec<RuntimeValue> {
        let gone: std::collections::HashSet<NodeId> = ids.iter().copied().collect();
        let mut values = Vec::with_capacity(ids.len());
        for &id in ids {
            let at = id.0 as usize;
            if let NodeKind::Derived { deps } = &self.nodes[at].kind {
                for dep in deps.clone() {
                    self.dependents[dep.0 as usize].retain(|&d| d != id);
                }
            }
            // A value outside that read this node no longer does.
            for dependent in std::mem::take(&mut self.dependents[at]) {
                if gone.contains(&dependent) {
                    continue;
                }
                if let NodeKind::Derived { deps } = &mut self.nodes[dependent.0 as usize].kind {
                    deps.retain(|&d| d != id);
                }
            }
            let node = std::mem::replace(
                &mut self.nodes[at],
                Node {
                    name: String::new(),
                    kind: NodeKind::Source,
                    value: RuntimeValue::Unit,
                },
            );
            if self.name_to_id.get(&node.name) == Some(&id) {
                self.name_to_id.remove(&node.name);
            }
            values.push(node.value);
            self.free.push(id);
        }
        // Removing edges keeps the order valid.
        values
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

        let ordered = self.ordered_before(&new_deps, id);
        self.nodes[id.0 as usize].kind = NodeKind::Derived { deps: new_deps };
        if !ordered {
            self.rebuild_topo();
        }
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

        self.topo_pos = vec![usize::MAX; n];
        for (i, id) in order.iter().enumerate() {
            self.topo_pos[id.0 as usize] = i;
        }
        self.topo_order = order;
        self.topo_stale = 0;
    }

    /// Returns `false` when dependency edges contain a cycle.
    pub fn is_acyclic(&self) -> bool {
        self.topo_order.len() == self.nodes.len()
    }

    fn propagate_from(&mut self, _changed: NodeId) {
        // Recomputation is handled by Executor.
    }
}
