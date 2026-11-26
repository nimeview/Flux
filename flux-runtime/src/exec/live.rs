//! Instances of `live fn` components.
//!
//! A component's members are compiled as template nodes, handlers, and
//! watches (`CompiledTemplate`), which the executor never runs as they are.
//! Calling the constructor makes an instance: graph nodes for its members,
//! its own watches, and subscriptions of its handlers to global events. Code
//! of the component runs with that instance as the *receiver*: a member's
//! name then reads or writes the receiver's node.
//!
//! A [`RuntimeValue::Instance`] is the only owner of an instance. Global
//! events and watches refer to it by number, so they do not keep it alive;
//! when its last handle is dropped, the mailbox notes it, and the executor
//! removes it at the end of the outermost batch (no code of it runs then).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use flux_ir::{NodeKind as IrNodeKind, StmtBlock};
use flux_value::{InstanceMailbox, InstanceRef};

use super::{Executor, Locals, LOCALS_POOL_LIMIT};
use crate::graph::{NodeId, RuntimeValue};

/// One instance of a component.
pub(super) struct Instance {
    pub(super) template: usize,
    /// Each member's node, by the member's internal name.
    pub(super) nodes: HashMap<String, NodeId>,
}

/// What the executor keeps about components and their instances.
#[derive(Default)]
pub(super) struct LiveState {
    pub(super) instances: HashMap<u64, Instance>,
    pub(super) next_id: u64,
    pub(super) mailbox: Arc<InstanceMailbox>,
    /// Per graph node: the instance it belongs to, if any.
    pub(super) node_receiver: Vec<Option<u64>>,
    /// Indices into `program.nodes` of the templates' members.
    pub(super) template_nodes: HashSet<usize>,
    /// The templates' events (internal names), with their handlers.
    pub(super) member_events: HashMap<String, Vec<usize>>,
    /// The templates' handlers of global events, by template.
    pub(super) global_handlers: Vec<Vec<usize>>,
    /// Instances' handlers of each global event, in the order the
    /// instances were made.
    pub(super) subscriptions: HashMap<String, Vec<(u64, usize)>>,
    /// Watches of instances made since watches last ran: they run once.
    pub(super) first_runs: bool,
}

impl LiveState {
    pub(super) fn new(program: &flux_ir::CompiledProgram) -> Self {
        let mut state = LiveState {
            next_id: 1,
            ..Default::default()
        };
        for template in &program.templates {
            state.template_nodes.extend(template.nodes.iter().copied());
            let mut global = Vec::new();
            for &h in &template.on_handlers {
                let event = &program.on_handlers[h].event;
                if template.events.contains(event) {
                    state
                        .member_events
                        .entry(event.clone())
                        .or_default()
                        .push(h);
                } else {
                    global.push(h);
                }
            }
            for event in &template.events {
                state.member_events.entry(event.clone()).or_default();
            }
            state.global_handlers.push(global);
        }
        state
    }

    /// The template handlers and watches, which the executor's own tables
    /// leave out.
    pub(super) fn template_handlers(
        program: &flux_ir::CompiledProgram,
    ) -> (HashSet<usize>, HashSet<usize>) {
        let mut handlers = HashSet::new();
        let mut watches = HashSet::new();
        for template in &program.templates {
            handlers.extend(template.on_handlers.iter().copied());
            watches.extend(template.watches.iter().copied());
        }
        (handlers, watches)
    }
}

impl Executor {
    /// The node a member name means for the current receiver.
    pub(super) fn member_node(&self, name: &str) -> Option<NodeId> {
        let receiver = self.receiver?;
        self.live.instances.get(&receiver)?.nodes.get(name).copied()
    }

    /// The node `name` means here: a global binding, or the receiver's member.
    pub(super) fn resolve_node(&self, name: &str) -> Option<NodeId> {
        self.graph
            .name_to_id
            .get(name)
            .copied()
            .or_else(|| self.member_node(name))
    }

    /// Whether instance `id` still exists: made, and its last handle not dropped.
    pub(super) fn is_alive(&self, id: u64) -> bool {
        self.live.instances.contains_key(&id) && !self.live.mailbox.contains(id)
    }

    /// Runs `f` with `receiver` as the instance member names refer to.
    pub(super) fn with_receiver<R>(
        &mut self,
        receiver: Option<u64>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved = std::mem::replace(&mut self.receiver, receiver);
        let result = f(self);
        self.receiver = saved;
        result
    }

    /// `a.n`: the current value of member `name` of instance `handle`, read
    /// like a binding (a dependency of the value being computed).
    pub(super) fn read_member(&mut self, handle: &RuntimeValue, name: &str) -> RuntimeValue {
        let node = match handle {
            RuntimeValue::Instance(instance) => self
                .live
                .instances
                .get(&instance.id)
                .and_then(|i| i.nodes.get(name).copied()),
            _ => None,
        };
        let Some(id) = node else {
            self.raise(flux_value::ArithError::Internal);
            return RuntimeValue::Unit;
        };
        self.record_dep(id);
        self.refresh(id);
        self.graph.get(id).clone()
    }

    /// `a.f(…)`: calls the component's function `name` on the instance that
    /// `args[0]` is.
    pub(super) fn call_member(&mut self, name: &str, args: &[RuntimeValue]) -> RuntimeValue {
        let Some(RuntimeValue::Instance(instance)) = args.first() else {
            self.raise(flux_value::ArithError::Internal);
            return RuntimeValue::Unit;
        };
        let id = instance.id;
        // `args` keeps the instance alive while its code runs.
        self.with_receiver(Some(id), |this| this.call_function(name, &args[1..]))
    }

    /// `emit a.Inc(…)`: runs instance `args[0]`'s handlers of `event`.
    pub(super) fn emit_member(&mut self, event: &str, args: Vec<RuntimeValue>) {
        let Some(RuntimeValue::Instance(instance)) = args.first() else {
            self.raise(flux_value::ArithError::Internal);
            return;
        };
        let id = instance.id;
        let values = args[1..].to_vec();
        self.with_receiver(Some(id), |this| this.emit_values(event, values));
        drop(args);
    }

    /// Runs the handlers of a component's own event `event`, for the
    /// current receiver. Returns `false` when `event` is not one.
    pub(super) fn run_member_handlers(&mut self, event: &str, values: &[RuntimeValue]) -> bool {
        let Some(handlers) = self.live.member_events.get(event).cloned() else {
            return false;
        };
        let Some(receiver) = self.receiver else {
            return true;
        };
        let opened = self.begin_batch();
        for idx in handlers {
            if !self.run_handler(idx, event, values, Some(receiver)) {
                break;
            }
        }
        self.end_batch(opened);
        true
    }

    /// Runs the handlers instances have for the global event `event`, in the
    /// order the instances were made; a dropped instance's are skipped.
    pub(super) fn run_instance_subscriptions(&mut self, event: &str, values: &[RuntimeValue]) {
        let Some(subs) = self.live.subscriptions.get(event).cloned() else {
            return;
        };
        for (instance, idx) in subs {
            if !self.is_alive(instance) {
                continue;
            }
            if !self.run_handler(idx, event, values, Some(instance)) {
                break;
            }
        }
    }

    /// Runs `on` handler `idx` with `values`; with a receiver, always on the
    /// VM. Returns `false` after a runtime error.
    pub(super) fn run_handler(
        &mut self,
        idx: usize,
        event: &str,
        values: &[RuntimeValue],
        receiver: Option<u64>,
    ) -> bool {
        let body: Arc<StmtBlock> = self.reactive.handler_bodies[idx].clone();
        let mut locals = self.locals_pool.pop().unwrap_or_default();
        // A handler's values are slots `0..n`.
        let named = self.program.on_handlers[idx].params.len().min(values.len());
        for (slot, value) in values[..named].iter().enumerate() {
            locals.set(slot as u32, value.clone());
        }
        let shown = flux_value::type_display_name(event).to_string();
        let shown = shown.rsplit("__").next().unwrap_or(&shown).to_string();
        let ran_native =
            receiver.is_none() && self.try_run_jit_on(idx, &body, &mut locals, &values[..named]);
        if !ran_native {
            self.with_receiver(receiver, |this| this.run_stmts(&body, &mut locals));
            if self.trap.is_some() {
                self.push_trace_frame(format!("on {shown}"), None);
            }
        }
        locals.clear();
        if self.locals_pool.len() < LOCALS_POOL_LIMIT {
            self.locals_pool.push(locals);
        }
        if self.trap.is_some() {
            self.set_trap_context(&format!("`on {shown}`"), None);
            return false;
        }
        true
    }

    /// Makes an instance of `templates[template]` from the constructor's
    /// arguments, and returns its handle.
    pub(super) fn construct(&mut self, template: usize, args: Vec<RuntimeValue>) -> RuntimeValue {
        let id = self.live.next_id;
        self.live.next_id += 1;
        let tmpl = self.program.templates[template].clone();
        let handle = RuntimeValue::Instance(Arc::new(InstanceRef {
            id,
            type_name: tmpl.name.clone(),
            mailbox: Arc::clone(&self.live.mailbox),
        }));
        let opened = self.begin_batch();

        // A node per member: sources start with their literal value (a
        // parameter with its argument), derived values are computed below.
        let mut nodes = HashMap::new();
        let mut proto_to_node: HashMap<usize, NodeId> = HashMap::new();
        let mut derived = Vec::new();
        for &proto in &tmpl.nodes {
            let ir = &self.program.nodes[proto];
            let ir_name = ir.name.clone();
            let value = match ir.kind {
                IrNodeKind::Source => Some(match tmpl.params.iter().position(|p| *p == ir.name) {
                    Some(i) => args.get(i).cloned().unwrap_or(RuntimeValue::Unit),
                    None => super::values::eval_const(&ir.init).unwrap_or(RuntimeValue::Unit),
                }),
                _ => None,
            };
            let is_derived = value.is_none();
            let (node, _) = self
                .graph
                .add_instance_node(format!("{ir_name}#{id}"), value);
            self.grow_node_tables();
            self.reactive.node_of[node.0 as usize] = is_derived.then_some(proto);
            self.live.node_receiver[node.0 as usize] = Some(id);
            nodes.insert(ir_name, node);
            proto_to_node.insert(proto, node);
            if is_derived {
                derived.push((node, proto));
            }
        }
        // A derived member depends on the instance's members and on globals.
        // One that records its reads as it runs (it may read other
        // instances) gets them from its first run.
        let edges = derived
            .iter()
            .map(|&(node, proto)| {
                if self.reactive.node_dynamic[proto] {
                    return (node, Vec::new());
                }
                let deps = self.program.nodes[proto]
                    .deps
                    .iter()
                    .filter_map(|d| {
                        let d = d.0 as usize;
                        match proto_to_node.get(&d) {
                            Some(&mine) => Some(mine),
                            None if self.live.template_nodes.contains(&d) => None,
                            None => Some(NodeId(d as u32)),
                        }
                    })
                    .collect();
                (node, deps)
            })
            .collect();
        self.graph.connect(edges);
        self.live.instances.insert(
            id,
            super::live::Instance {
                template,
                nodes: nodes.clone(),
            },
        );

        self.with_receiver(Some(id), |this| {
            for stmt in &tmpl.init_stmts {
                this.run_stmt(stmt, &mut Locals::default());
                if this.trap.is_some() {
                    this.push_trace_frame(
                        flux_value::type_display_name(&tmpl.name).to_string(),
                        None,
                    );
                    return;
                }
            }
        });
        // Its derived values, in dependency order.
        if self.trap.is_none() {
            let mut order: Vec<NodeId> = derived.iter().map(|(d, _)| *d).collect();
            order.sort_by_key(|n| self.graph.topo_position(*n));
            for node in &order {
                let at = node.0 as usize;
                if !self.reactive.stale[at] {
                    self.reactive.stale[at] = true;
                    self.reactive.stale_count += 1;
                }
            }
            for node in order {
                self.ensure_current(node);
                if self.trap.is_some() {
                    break;
                }
            }
        }

        // Handlers of global events, and watches.
        for &h in &self.live.global_handlers[template].clone() {
            let event = self.program.on_handlers[h].event.clone();
            self.live
                .subscriptions
                .entry(event)
                .or_default()
                .push((id, h));
        }
        for &w in &tmpl.watches {
            let watch = &self.program.watches[w];
            let graph = &self.graph;
            let find = |name: &String| {
                nodes
                    .get(name)
                    .copied()
                    .or_else(|| graph.name_to_id.get(name).copied())
            };
            let triggers: Vec<NodeId> = match &watch.target {
                Some(target) => find(target).into_iter().collect(),
                None => watch.deps.iter().filter_map(find).collect(),
            };
            self.reactive.watch_triggers.push((w, triggers));
            self.reactive.watch_receivers.push(Some(id));
            self.reactive.watch_memory.push(None);
        }
        self.live.first_runs = true;
        self.end_batch(opened);
        handle
    }

    /// Whether storing `value` in a member of instance `owner` would make the
    /// instance hold itself: `value` holds it, or holds an instance that
    /// holds it (at any depth).
    pub(super) fn would_hold_itself(&self, owner: u64, value: &RuntimeValue) -> bool {
        let mut stack = Vec::new();
        instances_in(value, &mut stack);
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if id == owner {
                return true;
            }
            if !seen.insert(id) {
                continue;
            }
            if let Some(instance) = self.live.instances.get(&id) {
                for node in instance.nodes.values() {
                    instances_in(self.graph.get(*node), &mut stack);
                }
            }
        }
        false
    }

    /// Checks a value about to be stored in node `id`: an instance's member
    /// may not come to hold the instance. Raises the error and returns
    /// `false` if it would.
    pub(super) fn may_store(&mut self, id: NodeId, value: &RuntimeValue) -> bool {
        let Some(owner) = self
            .live
            .node_receiver
            .get(id.0 as usize)
            .copied()
            .flatten()
        else {
            return true;
        };
        if self.would_hold_itself(owner, value) {
            self.raise(flux_value::ArithError::InstanceHoldsItself);
            return false;
        }
        true
    }

    /// Makes the per-node tables as long as the graph.
    pub(super) fn grow_node_tables(&mut self) {
        let n = self.graph.nodes.len();
        let r = &mut self.reactive;
        if r.node_of.len() < n {
            r.node_of.resize(n, None);
            r.stale.resize(n, false);
            r.changed_marks.resize(n, false);
            r.reached.resize(n, false);
            r.evaluating.resize(n, false);
        }
        if self.live.node_receiver.len() < n {
            self.live.node_receiver.resize(n, None);
        }
    }

    /// Removes the instances whose last handle was dropped, and those only
    /// they held. Runs at the end of the outermost batch.
    pub(super) fn drop_dead_instances(&mut self) {
        loop {
            let dead = self.live.mailbox.take();
            if dead.is_empty() {
                return;
            }
            for id in dead {
                let Some(instance) = self.live.instances.remove(&id) else {
                    continue;
                };
                let _ = instance.template;
                let ids: Vec<NodeId> = instance.nodes.values().copied().collect();
                for &node in &ids {
                    let at = node.0 as usize;
                    if self.reactive.stale[at] {
                        self.reactive.stale[at] = false;
                        self.reactive.stale_count -= 1;
                    }
                    self.reactive.node_of[at] = None;
                    self.live.node_receiver[at] = None;
                }
                self.reactive.changed_nodes.retain(|n| !ids.contains(n));
                self.reactive.mutated_sources.retain(|n| !ids.contains(n));
                for subs in self.live.subscriptions.values_mut() {
                    subs.retain(|(owner, _)| *owner != id);
                }
                let receivers = std::mem::take(&mut self.reactive.watch_receivers);
                let triggers = std::mem::take(&mut self.reactive.watch_triggers);
                let memory = std::mem::take(&mut self.reactive.watch_memory);
                for ((receiver, trigger), seen) in receivers.into_iter().zip(triggers).zip(memory) {
                    if receiver != Some(id) {
                        self.reactive.watch_receivers.push(receiver);
                        self.reactive.watch_triggers.push(trigger);
                        self.reactive.watch_memory.push(seen);
                    }
                }
                // Dropping its values may drop instances only it held.
                let values = self.graph.remove_nodes(&ids);
                drop(values);
            }
        }
    }
}

/// The numbers of the instances `value` holds, at any depth.
fn instances_in(value: &RuntimeValue, out: &mut Vec<u64>) {
    stacker::maybe_grow(64 * 1024, 1024 * 1024, || match value {
        RuntimeValue::Instance(instance) => out.push(instance.id),
        RuntimeValue::Array(items) => items.iter().for_each(|v| instances_in(v, out)),
        RuntimeValue::UserStruct(s) => s.fields.values().for_each(|v| instances_in(v, out)),
        RuntimeValue::UserEnum(e) => {
            if let Some(p) = &e.payload {
                instances_in(p, out);
            }
        }
        RuntimeValue::Closure(c) => c.captured.iter().for_each(|v| instances_in(v, out)),
        RuntimeValue::Option(Some(v))
        | RuntimeValue::Result(Ok(v))
        | RuntimeValue::Result(Err(v)) => instances_in(v, out),
        RuntimeValue::Map(entries) => entries.values().for_each(|(_, v)| instances_in(v, out)),
        _ => {}
    })
}

/// `value` with each instance it holds replaced by its number, for a
/// comparison that does not keep the instances alive.
pub(super) fn without_handles(value: &RuntimeValue) -> RuntimeValue {
    let mut found = Vec::new();
    instances_in(value, &mut found);
    if found.is_empty() {
        return value.clone();
    }
    replace_handles(value)
}

fn replace_handles(value: &RuntimeValue) -> RuntimeValue {
    stacker::maybe_grow(64 * 1024, 1024 * 1024, || match value {
        RuntimeValue::Instance(instance) => {
            RuntimeValue::String(format!("\0instance#{}", instance.id))
        }
        RuntimeValue::Array(items) => {
            RuntimeValue::array(items.iter().map(replace_handles).collect())
        }
        RuntimeValue::UserStruct(s) => RuntimeValue::user_struct(
            s.type_name.clone(),
            s.fields
                .iter()
                .map(|(k, v)| (k.clone(), replace_handles(v)))
                .collect(),
        ),
        RuntimeValue::UserEnum(e) => RuntimeValue::user_enum(
            e.type_name.clone(),
            e.variant.clone(),
            e.payload.as_ref().map(replace_handles),
        ),
        RuntimeValue::Closure(c) => RuntimeValue::Closure(Arc::new(flux_value::ClosureValue {
            function: c.function.clone(),
            captured: c.captured.iter().map(replace_handles).collect(),
        })),
        RuntimeValue::Option(Some(v)) => RuntimeValue::Option(Some(Box::new(replace_handles(v)))),
        RuntimeValue::Result(Ok(v)) => RuntimeValue::Result(Ok(Box::new(replace_handles(v)))),
        RuntimeValue::Result(Err(v)) => RuntimeValue::Result(Err(Box::new(replace_handles(v)))),
        RuntimeValue::Map(entries) => RuntimeValue::Map(Arc::new(
            entries
                .iter()
                .map(|(k, (key, v))| (k.clone(), (key.clone(), replace_handles(v))))
                .collect(),
        )),
        other => other.clone(),
    })
}
