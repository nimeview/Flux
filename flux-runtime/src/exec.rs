//! Reactive execution engine.
//!
//! [`Executor`] owns the reactive graph and compiled program and drives change
//! propagation. The interpreter itself is split into submodules that extend
//! `Executor` with more `impl` blocks (they can touch private fields because
//! they are descendants of this module):
//! - [`stmts`]  — statement execution + control flow
//! - [`eval`]   — stack-machine expression evaluation + function dispatch
//! - [`values`] — pure value helpers (operators, conversions, predicates)

mod eval;
mod live;
mod place;
mod stmts;
mod timers;
mod values;

pub use timers::{EventSender, Host, HostFn};

use std::collections::{HashMap, HashSet};

use flux_ir::{CompiledFunction, CompiledProgram, HandlerKind, NodeKind as IrNodeKind};

use crate::backend::{ActiveBackend, EvalContext, ExecBackend};
use crate::error::{ExecutorError, RuntimeError, TraceFrame};
use crate::graph::{NodeId, NodeKind, ReactiveGraph, RuntimeValue};
use crate::vm::validate_program;
use crate::ExecutionMode;

use values::{code_has_lazy_if, eval_const};

/// Whether a handler's or watch's body has a loop: only then is it worth
/// running compiled.
fn has_loop(body: &[flux_ir::IrStmt]) -> bool {
    use flux_ir::IrStmt;
    body.iter().any(|stmt| match stmt {
        IrStmt::While { .. } | IrStmt::For { .. } => true,
        IrStmt::If {
            then_body,
            else_body,
            ..
        } => has_loop(then_body) || else_body.as_ref().is_some_and(|b| has_loop(b)),
        _ => false,
    })
}

/// A frame of local bindings, indexed by the slots `flux_ir::resolve_locals`
/// assigns. A slot is unbound (`None`) until its binding runs.
#[derive(Default)]
pub(crate) struct Locals {
    slots: Vec<Option<RuntimeValue>>,
}

impl Locals {
    pub(crate) fn get(&self, slot: u32) -> Option<&RuntimeValue> {
        self.slots.get(slot as usize)?.as_ref()
    }

    pub(crate) fn take(&mut self, slot: u32) -> Option<RuntimeValue> {
        self.slots.get_mut(slot as usize)?.take()
    }

    pub(crate) fn set(&mut self, slot: u32, value: RuntimeValue) {
        let i = slot as usize;
        if i >= self.slots.len() {
            self.slots.resize_with(i + 1, || None);
        }
        self.slots[i] = Some(value);
    }

    /// Unbinds every slot, keeping the allocation for the next call.
    fn clear(&mut self) {
        self.slots.clear();
    }
}

/// Frames kept for reuse; deeper recursion allocates beyond this.
const LOCALS_POOL_LIMIT: usize = 64;

#[cfg(feature = "jit")]
fn tagged_to_runtime(tv: flux_value::TaggedVal) -> RuntimeValue {
    flux_value::tagged_to_value(tv)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlowControl {
    None,
    Return,
    Break,
    Continue,
}

struct StmtResult {
    value: RuntimeValue,
    flow: FlowControl,
}

impl From<(RuntimeValue, FlowControl)> for StmtResult {
    fn from((value, flow): (RuntimeValue, FlowControl)) -> Self {
        StmtResult { value, flow }
    }
}

/// What event dispatch and settling need: tables fixed at start, and buffers
/// they reuse so an event does not allocate.
struct ReactiveState {
    mutated_sources: Vec<NodeId>,
    /// Per graph node: in `mutated_sources` (a batch may write a thousand).
    mutated_marks: Vec<bool>,
    changed_nodes: Vec<NodeId>,
    /// Spare buffer for `changed_nodes`, empty between watch rounds.
    changed_spare: Vec<NodeId>,
    /// Per graph node: set only while a watch round tests what changed.
    changed_marks: Vec<bool>,
    /// Per graph node: its index in `program.nodes`, if it is a source or derived value.
    node_of: Vec<Option<usize>>,
    /// Per `program.nodes` entry: its dependencies are recorded as it runs.
    node_dynamic: Vec<bool>,
    /// Handler indices per event, in graph order.
    event_handlers: HashMap<String, std::sync::Arc<[usize]>>,
    /// Handler bodies, shared so an event does not copy them.
    handler_bodies: Vec<std::sync::Arc<flux_ir::StmtBlock>>,
    handler_keys: Vec<String>,
    /// Watch bodies, shared so a run does not copy them.
    watch_bodies: Vec<std::sync::Arc<flux_ir::StmtBlock>>,
    watch_keys: Vec<String>,
    /// Runnable watches in graph order, with the nodes that trigger each:
    /// the modules', then each instance's, in the order instances were made.
    watch_triggers: Vec<(usize, Vec<NodeId>)>,
    /// Per entry of `watch_triggers`: the instance it belongs to, if any.
    watch_receivers: Vec<Option<u64>>,
    /// Per entry of `watch_triggers`: the values of its triggers when it last
    /// ran, or `None` before its first run. A watch runs only when one of
    /// them differs.
    watch_memory: Vec<Option<Vec<RuntimeValue>>>,
    /// Reused while finding what a write dirtied.
    /// Per graph node: a source it reads has changed since it was computed. Set
    /// at the write, cleared when the value is recomputed, by a read or a flush.
    stale: Vec<bool>,
    /// How many nodes are stale; zero is the common case and skips all checks.
    stale_count: usize,
    /// Scratch marks for what a flush's sources reach.
    reached: Vec<bool>,
    /// Per graph node: being computed now. Reading one then is a cycle.
    evaluating: Vec<bool>,
    /// How many values `ensure_current` is computing, one inside another.
    eval_depth: u32,
    /// The program has components. Only then can a value read itself at run
    /// time, or a chain of values be deep, so only then are they checked.
    components: bool,
    /// While modules initialize (in a program with components): the sources
    /// whose computed initial value is not set yet.
    unset: HashSet<NodeId>,
    dirty_stack: Vec<NodeId>,
    dirty_order: Vec<NodeId>,
}

pub struct Executor {
    pub graph: ReactiveGraph,
    pub program: CompiledProgram,
    /// Shared so a call can run a body without cloning it.
    /// Looked up on every call: a fast hash, not SipHash.
    functions: rustc_hash::FxHashMap<String, std::sync::Arc<CompiledFunction>>,
    /// `program.nodes` index by binding name.
    node_index: HashMap<String, usize>,
    /// Each node's code, shared so a recomputation does not copy it.
    node_code: Vec<std::sync::Arc<[flux_ir::Inst]>>,
    /// Functions that may read reactive values (see `flux_ir::graph_readers`).
    graph_readers: rustc_hash::FxHashSet<String>,
    in_watch: bool,
    /// When true, `set_source` records mutations and skips propagation until flush.
    defer_propagation: bool,
    /// Kept apart so it does not spread the fields calls use over more cache lines.
    reactive: Box<ReactiveState>,
    backend: ActiveBackend,
    /// One entry per derived value being computed, innermost last: the graph
    /// reads it records when its dependencies are dynamic, or `None`. A stack,
    /// because reading a stale value computes it in the middle of another.
    tracking: Vec<Option<Vec<NodeId>>>,
    /// First runtime error; once set, evaluation unwinds and nothing else runs.
    trap: Option<Box<RuntimeError>>,
    /// Compiled code failed: the VM reruns it, and runs everything from then on.
    vm_only: bool,
    /// While modules initialize: the watches (by index) that may run, those
    /// of the modules initialized so far.
    initial_watches: Option<HashSet<usize>>,
    /// Operand stacks reused across expression evaluations.
    stack_pool: Vec<Vec<RuntimeValue>>,
    /// Cleared frames reused by calls: allocating one per call dominated
    /// call-heavy loops.
    locals_pool: Vec<Locals>,
    /// `return`/`break`/`continue` raised inside an expression (a match arm
    /// block), on its way to the enclosing statement.
    pending_flow: Option<(RuntimeValue, FlowControl)>,
    /// Active user function calls (bounded by `flux_value::MAX_CALL_DEPTH`).
    call_depth: u64,
    /// Components and their instances.
    live: Box<live::LiveState>,
    /// The instance whose component code is running, if any: member names
    /// mean its members.
    receiver: Option<u64>,
    /// What `after` and `every` set, and events sent from outside. Boxed,
    /// like `reactive`, to keep the fields calls use close together.
    timers: Box<timers::Timers>,
    /// The host's functions for the program's `extern fn`s.
    host: Box<timers::Host>,
}

impl Executor {
    /// Create an executor with the default VM engine.
    pub fn new(program: CompiledProgram) -> Result<Self, ExecutorError> {
        Self::with_mode(program, ExecutionMode::Vm)
    }

    /// Create an executor with an explicit engine choice.
    pub fn with_mode(program: CompiledProgram, mode: ExecutionMode) -> Result<Self, ExecutorError> {
        Self::with_host(program, mode, Host::default())
    }

    /// Create an executor for a host that gives the program's `extern fn`s.
    /// Every one the program declares must be provided.
    pub fn with_host(
        mut program: CompiledProgram,
        mode: ExecutionMode,
        host: Host,
    ) -> Result<Self, ExecutorError> {
        let mut missing: Vec<String> = program
            .functions
            .iter()
            .flat_map(|f| f.body.iter())
            .filter_map(|stmt| match stmt {
                flux_ir::IrStmt::Return { expr: Some(code) } => code.iter().find_map(|i| match i {
                    flux_ir::Inst::Call { name, .. } => name.strip_prefix(flux_ir::EXTERN_PREFIX),
                    _ => None,
                }),
                _ => None,
            })
            .filter(|name| !host.functions.contains_key(*name))
            .map(str::to_string)
            .collect();
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(ExecutorError::MissingHostFunctions(missing));
        }
        // Lowering already resolves slots; this covers IR built elsewhere.
        flux_ir::resolve_locals(&mut program);
        validate_program(&program).map_err(ExecutorError::Invalid)?;

        let functions = program
            .functions
            .iter()
            .map(|f| (f.name.clone(), std::sync::Arc::new(f.clone())))
            .collect();

        let mut graph = ReactiveGraph::new();
        let mut pending_derived_deps = Vec::new();
        // A component's members are templates: only instances are wired.
        let live_state = live::LiveState::new(&program);
        let wired = |i: usize, d: &flux_ir::NodeId| {
            !live_state.template_nodes.contains(&i)
                && !live_state.template_nodes.contains(&(d.0 as usize))
        };
        for (i, node) in program.nodes.iter().enumerate() {
            match node.kind {
                IrNodeKind::Source => {
                    let value = eval_const(&node.init).unwrap_or(RuntimeValue::Unit);
                    graph.add_source(&node.name, value);
                }
                IrNodeKind::Derived => {
                    let deps: Vec<_> = node
                        .deps
                        .iter()
                        .filter(|d| wired(i, d))
                        .map(|d| crate::graph::NodeId(d.0))
                        .collect();
                    let id = graph.add_derived(&node.name, Vec::new());
                    pending_derived_deps.push((id, deps));
                }
                IrNodeKind::Event => {
                    graph.add_event(&node.name);
                }
                IrNodeKind::Effect => {
                    let meta = node
                        .effect
                        .clone()
                        .expect("effect IR node must carry EffectMeta");
                    let deps: Vec<_> = node
                        .deps
                        .iter()
                        .map(|d| crate::graph::NodeId(d.0))
                        .collect();
                    graph.add_effect(&node.name, deps, meta);
                }
            }
        }

        for (id, deps) in pending_derived_deps {
            graph.set_derived_deps(id, deps);
        }

        if !graph.is_acyclic() {
            return Err(ExecutorError::Invalid(
                "reactive graph contains a dependency cycle".to_string(),
            ));
        }

        let (template_handlers, template_watches) = live::LiveState::template_handlers(&program);
        for &proto in &live_state.template_nodes {
            graph.name_to_id.remove(&program.nodes[proto].name);
        }
        for event in live_state.member_events.keys() {
            graph.name_to_id.remove(event);
        }
        let node_index: HashMap<String, usize> = program
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, n)| {
                matches!(n.kind, IrNodeKind::Source | IrNodeKind::Derived)
                    && !live_state.template_nodes.contains(i)
            })
            .map(|(i, n)| (n.name.clone(), i))
            .collect();
        let node_code = program
            .nodes
            .iter()
            .map(|n| std::sync::Arc::from(n.code.as_slice()))
            .collect();

        // A value that may read an instance's member (directly, or through a
        // function) records what it reads as it runs.
        let has_templates = !program.templates.is_empty();
        let node_dynamic = program
            .nodes
            .iter()
            .map(|n| {
                code_has_lazy_if(&n.code)
                    || (has_templates && values::code_may_read_members(&n.code))
            })
            .collect();
        let node_of = graph
            .nodes
            .iter()
            .map(|n| node_index.get(&n.name).copied())
            .collect();
        let mut event_handlers: HashMap<String, std::sync::Arc<[usize]>> = HashMap::new();
        for (name, &id) in &graph.name_to_id {
            let handlers: Vec<usize> = graph
                .effects_for_event(id)
                .iter()
                .filter_map(|&effect_id| match &graph.nodes[effect_id.0 as usize].kind {
                    NodeKind::Effect { meta }
                        if meta.handler_kind == HandlerKind::On
                            && !template_handlers.contains(&meta.handler_index) =>
                    {
                        Some(meta.handler_index)
                    }
                    _ => None,
                })
                .collect();
            if !handlers.is_empty() {
                event_handlers.insert(name.clone(), handlers.into());
            }
        }
        let watch_triggers: Vec<(usize, Vec<NodeId>)> = graph
            .nodes
            .iter()
            .filter_map(|node| {
                let NodeKind::Effect { meta } = &node.kind else {
                    return None;
                };
                if meta.handler_kind != HandlerKind::Watch
                    || template_watches.contains(&meta.handler_index)
                {
                    return None;
                }
                let watch = &program.watches[meta.handler_index];
                let triggers = match &watch.target {
                    Some(target) => vec![*graph.name_to_id.get(target)?],
                    None => watch
                        .deps
                        .iter()
                        .filter_map(|dep| graph.name_to_id.get(dep).copied())
                        .collect(),
                };
                Some((meta.handler_index, triggers))
            })
            .collect();
        let handler_bodies = program
            .on_handlers
            .iter()
            .map(|h| std::sync::Arc::new(h.body.clone()))
            .collect();
        let handler_keys = (0..program.on_handlers.len())
            .map(|i| format!("__on_{i}"))
            .collect();
        let watch_bodies = program
            .watches
            .iter()
            .map(|w| std::sync::Arc::new(w.body.clone()))
            .collect();
        let watch_keys = (0..program.watches.len())
            .map(|i| format!("__watch_{i}"))
            .collect();
        let node_count = graph.nodes.len();
        let module_watch_count = watch_triggers.len();
        let graph_readers =
            flux_ir::graph_readers(&program, |name| graph.name_to_id.contains_key(name));

        let backend = ActiveBackend::from_mode(mode, &program);

        let mut this = Self {
            graph,
            program,
            functions,
            node_index,
            node_code,
            graph_readers: graph_readers.into_iter().collect(),
            in_watch: false,
            defer_propagation: false,
            reactive: Box::new(ReactiveState {
                mutated_sources: Vec::new(),
                mutated_marks: vec![false; node_count],
                changed_nodes: Vec::new(),
                changed_spare: Vec::new(),
                changed_marks: vec![false; node_count],
                node_of,
                node_dynamic,
                event_handlers,
                handler_bodies,
                handler_keys,
                watch_bodies,
                watch_keys,
                watch_triggers,
                watch_receivers: vec![None; module_watch_count],
                watch_memory: vec![None; module_watch_count],
                stale: vec![false; node_count],
                stale_count: 0,
                reached: vec![false; node_count],
                evaluating: vec![false; node_count],
                eval_depth: 0,
                components: has_templates,
                unset: HashSet::new(),
                dirty_stack: Vec::new(),
                dirty_order: Vec::new(),
            }),
            backend,
            tracking: Vec::new(),
            stack_pool: Vec::new(),
            locals_pool: Vec::new(),
            pending_flow: None,
            call_depth: 0,
            live: Box::new(live::LiveState {
                node_receiver: vec![None; node_count],
                ..live_state
            }),
            receiver: None,
            timers: Box::default(),
            host: Box::new(host),
            trap: None,
            vm_only: false,
            initial_watches: None,
        };
        // Modules initialize one after another, dependencies first. A
        // module's initial values are assigned without propagation; then
        // derived values recompute and the watches of the modules so far run
        // (a watch that ran before runs again only if what it follows
        // changed). Without groups: all statements, then all watches.
        let stmts = this.program.init_stmts.clone();
        let groups = if this.program.init_groups.is_empty() {
            vec![flux_ir::InitGroup {
                stmts_end: stmts.len(),
                watches: (0..this.program.watches.len()).collect(),
                derived: Vec::new(),
            }]
        } else {
            this.program.init_groups.clone()
        };
        // An instance made while modules initialize computes its derived
        // values at once, and they may read the modules' derived values:
        // those are computed when first read (and all of them at the end).
        if !this.program.templates.is_empty() {
            for stmt in &stmts {
                if let flux_ir::IrStmt::Assign {
                    target, slot: None, ..
                } = stmt
                {
                    if let Some(&id) = this.graph.name_to_id.get(target) {
                        this.reactive.unset.insert(id);
                    }
                }
            }
            for at in 0..this.graph.nodes.len() {
                let derived = matches!(this.graph.nodes[at].kind, NodeKind::Derived { .. });
                if derived && this.reactive.node_of[at].is_some() && !this.reactive.stale[at] {
                    this.reactive.stale[at] = true;
                    this.reactive.stale_count += 1;
                }
            }
        }
        let mut start = 0;
        let mut scope: HashSet<usize> = HashSet::new();
        let last = groups.len() - 1;
        for (g, group) in groups.iter().enumerate() {
            this.defer_propagation = true;
            for stmt in &stmts[start..group.stmts_end] {
                this.run_stmt(stmt, &mut Locals::default());
                if this.trap.is_some() {
                    let binding = match stmt {
                        flux_ir::IrStmt::Assign { target, .. } => Some(target.as_str()),
                        _ => None,
                    };
                    let declared = binding.and_then(|b| this.program.source_map.bindings.get(b));
                    let declared = declared.copied();
                    this.push_trace_frame("module initialization".to_string(), declared);
                    this.set_trap_context("module initialization", binding);
                    break;
                }
            }
            start = group.stmts_end;
            this.defer_propagation = false;
            for id in std::mem::take(&mut this.reactive.mutated_sources) {
                this.reactive.mutated_marks[id.0 as usize] = false;
            }
            this.reactive.changed_nodes.clear();
            if this.trap.is_some() {
                break;
            }
            scope.extend(group.watches.iter().copied());
            // Later modules' derived values wait for their statements.
            if g == last {
                this.recompute_all();
            } else {
                let ids: Vec<NodeId> = group.derived.iter().map(|id| NodeId(id.0)).collect();
                this.recompute(&ids);
            }
            this.initial_watches = Some(scope.clone());
            this.run_watches();
            this.initial_watches = None;
            if this.trap.is_some() {
                break;
            }
        }
        this.reactive.unset.clear();
        if let Some(err) = this.trap.take() {
            return Err(ExecutorError::Runtime(*err));
        }
        this.drop_dead_instances();
        Ok(this)
    }

    /// The first runtime error raised, if any. Once set, the executor stops
    /// running code; values computed before the error stay readable.
    pub fn runtime_error(&self) -> Option<&RuntimeError> {
        self.trap.as_deref()
    }

    pub(crate) fn raise(&mut self, kind: flux_value::ArithError) {
        if self.trap.is_none() {
            let detail = flux_std::take_failure_detail();
            self.trap = Some(Box::new(RuntimeError {
                kind,
                context: None,
                binding: None,
                detail,
                trace: Vec::new(),
                pending_loc: None,
            }));
        }
    }

    /// Attach `context` (and the binding, if any) to a pending runtime error
    /// that has none yet; the innermost caller that knows wins.
    fn set_trap_context(&mut self, context: &str, binding: Option<&str>) {
        if let Some(err) = &mut self.trap {
            if err.context.is_none() {
                err.context = Some(context.to_string());
                err.binding = binding.map(str::to_string);
            }
        }
    }

    /// Adds what was running to the pending error's trace, at the location
    /// its statements noted, or else at `fallback`.
    #[cold]
    #[inline(never)]
    pub(super) fn push_trace_frame(&mut self, name: String, fallback: Option<flux_ir::SourceLoc>) {
        let Some(err) = &mut self.trap else {
            return;
        };
        let location = err.pending_loc.take().or(fallback).and_then(|loc| {
            let (path, line, column) = self.program.source_map.resolve(loc)?;
            Some(format!("{path}:{line}:{column}"))
        });
        err.trace.push(TraceFrame { name, location });
    }

    /// Compiled code has no effects until it succeeds and keeps no locations,
    /// so a failure is discarded and the VM, the reference, runs it again and
    /// reports the error.
    pub(super) fn replay_on_vm(&mut self) {
        self.vm_only = true;
        let _ = flux_std::take_failure_detail();
    }

    /// Parameter count of the user function `name`, if the program defines it.
    pub fn function_arity(&self, name: &str) -> Option<usize> {
        self.functions
            .get(self.program.function_name(name))
            .map(|f| f.params.len())
    }

    /// Calls the user function `name` (e.g. a program's `main`). A runtime
    /// error raised inside it, or already pending, is returned as `Err`.
    pub fn call(
        &mut self,
        name: &str,
        args: &[RuntimeValue],
    ) -> Result<RuntimeValue, RuntimeError> {
        if let Some(err) = &self.trap {
            return Err((**err).clone());
        }
        let runtime_name = self.program.function_name(name).to_string();
        let value = self.call_function(&runtime_name, args);
        self.set_trap_context(&format!("`{name}`"), None);
        if self.trap.is_none() && !self.defer_propagation && !self.in_watch {
            self.drop_dead_instances();
        }
        match &self.trap {
            Some(err) => Err((**err).clone()),
            None => Ok(value),
        }
    }

    pub fn get_number(&self, name: &str) -> Option<f64> {
        self.graph.get_by_name(name).and_then(|v| v.as_float())
    }

    pub fn get_value(&self, name: &str) -> Option<RuntimeValue> {
        self.graph.get_by_name(name).cloned()
    }

    pub fn execution_mode(&self) -> ExecutionMode {
        self.backend.mode()
    }

    #[cfg(feature = "jit")]
    pub fn jit_stats(&self) -> Option<flux_jit::JitStats> {
        self.backend.jit_stats()
    }

    #[cfg(not(feature = "jit"))]
    pub fn jit_stats(&self) -> Option<()> {
        None
    }

    pub fn get_string(&self, name: &str) -> Option<String> {
        match self.graph.get_by_name(name)? {
            RuntimeValue::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Emit an event that carries at most one value, by name (for tests and
    /// host integration); `RuntimeValue::Unit` for an event without one.
    pub fn emit(&mut self, event: &str, payload: RuntimeValue) {
        self.emit_values(event, vec![payload]);
    }

    /// Emit an event with its values in order.
    pub fn emit_values(&mut self, event: &str, values: Vec<RuntimeValue>) {
        if self.trap.is_some() {
            return;
        }
        // A handler that emits nests like a call, under the same limit.
        if self.call_depth >= flux_value::MAX_CALL_DEPTH {
            self.raise(flux_value::ArithError::CallDepthExceeded);
            return;
        }
        self.call_depth += 1;
        stacker::maybe_grow(64 * 1024, 2 * 1024 * 1024, || {
            self.run_event_handlers(event, &values)
        });
        self.call_depth -= 1;
    }

    fn run_event_handlers(&mut self, event: &str, values: &[RuntimeValue]) {
        // A component's own event: the receiver's handlers.
        if self.run_member_handlers(event, values) {
            return;
        }
        let handlers = self.reactive.event_handlers.get(event).cloned();
        let subscribed =
            !self.live.subscriptions.is_empty() && self.live.subscriptions.contains_key(event);
        if handlers.is_none() && !subscribed {
            return;
        }
        let opened = self.begin_batch();
        // The modules' handlers, then the instances'.
        let mut ok = true;
        for &idx in handlers.iter().flat_map(|h| h.iter()) {
            if !self.run_handler(idx, event, values, None) {
                ok = false;
                break;
            }
        }
        if ok && subscribed {
            self.run_instance_subscriptions(event, values);
        }
        self.end_batch(opened);
    }

    /// Opens a batch unless one is open: an `emit` or an `act fn` call outside
    /// any handler, watch, `act fn`, or initialization. Writes in a batch apply
    /// at once but settle together when it ends. Returns whether this call
    /// opened it.
    pub(super) fn begin_batch(&mut self) -> bool {
        if self.defer_propagation || self.in_watch {
            return false;
        }
        self.defer_propagation = true;
        true
    }

    /// Ends the batch that `begin_batch` opened: recomputes what its writes
    /// made stale, then runs the watches. After a failure, the writes made
    /// before it stay and nothing else runs.
    pub(super) fn end_batch(&mut self, opened: bool) {
        if !opened {
            return;
        }
        self.defer_propagation = false;
        if self.trap.is_none() {
            self.flush_propagation();
        }
        if self.trap.is_none() {
            self.drop_dead_instances();
        }
    }

    pub fn set_source(&mut self, name: &str, value: RuntimeValue) {
        if self.trap.is_some() {
            return;
        }
        let Some(id) = self.resolve_node(name) else {
            return;
        };
        if !matches!(self.graph.nodes[id.0 as usize].kind, NodeKind::Source) {
            return;
        }
        if self.reactive.components {
            if !self.may_store(id, &value) {
                return;
            }
            if !self.reactive.unset.is_empty() {
                self.reactive.unset.remove(&id);
            }
        }
        self.graph.set_source(id, value);
        self.mark_stale(id);
        // A handler or watch body writes as a batch, applied when it ends.
        if self.defer_propagation || self.in_watch {
            let mark = &mut self.reactive.mutated_marks[id.0 as usize];
            if !*mark {
                *mark = true;
                self.reactive.mutated_sources.push(id);
            }
            return;
        }
        self.propagate_source_change(id);
    }

    fn propagate_source_change(&mut self, id: NodeId) {
        self.reactive.changed_nodes.push(id);
        self.recompute_from(&[id]);
        if !self.in_watch {
            self.run_watches();
        }
    }

    fn flush_propagation(&mut self) {
        if self.reactive.mutated_sources.is_empty() {
            // New instances' watches run once.
            if self.live.first_runs && !self.in_watch {
                self.run_watches();
            }
            return;
        }
        let mut sources = std::mem::take(&mut self.reactive.mutated_sources);
        for id in &sources {
            self.reactive.mutated_marks[id.0 as usize] = false;
        }
        self.reactive.changed_nodes.extend_from_slice(&sources);
        self.recompute_from(&sources);
        sources.clear();
        self.reactive.mutated_sources = sources;
        if !self.in_watch {
            self.run_watches();
        }
    }

    /// Computes the derived values `ids` (a module's), in dependency order.
    fn recompute(&mut self, ids: &[NodeId]) {
        let wanted: HashSet<NodeId> = ids.iter().copied().collect();
        let derived: Vec<_> = self
            .graph
            .topo()
            .filter(|id| wanted.contains(id))
            .filter_map(|id| self.reactive.node_of[id.0 as usize].map(|idx| (id, idx)))
            .collect();
        for (id, idx) in derived {
            let value = self.eval_derived(id, idx);
            if self.trap.is_some() {
                self.note_derived_failure(id);
                return;
            }
            self.graph.nodes[id.0 as usize].value = value;
            self.clear_stale(id);
        }
    }

    fn recompute_all(&mut self) {
        let derived: Vec<_> = self
            .graph
            .topo()
            .filter_map(|id| {
                let node = &self.graph.nodes[id.0 as usize];
                if !matches!(node.kind, NodeKind::Derived { .. }) {
                    return None;
                }
                self.reactive.node_of[id.0 as usize].map(|idx| (id, idx))
            })
            .collect();
        for (id, idx) in derived {
            let value = self.eval_derived(id, idx);
            if self.trap.is_some() {
                self.note_derived_failure(id);
                return;
            }
            self.graph.nodes[id.0 as usize].value = value;
            self.clear_stale(id);
        }
    }

    #[cold]
    fn note_derived_failure(&mut self, id: NodeId) {
        let name = self.graph.nodes[id.0 as usize].name.clone();
        let proto = name.split('#').next().unwrap_or(&name).to_string();
        let declared = self.program.source_map.bindings.get(&proto).copied();
        let shown = flux_value::binding_display_name(&name);
        self.push_trace_frame(shown.clone(), declared);
        self.set_trap_context(&format!("`{shown}`"), Some(&proto));
    }

    /// Evaluate one derived node via the active backend, falling back to VM bytecode.
    fn eval_derived(&mut self, id: NodeId, idx: usize) -> RuntimeValue {
        if !self.reactive.components {
            return self.eval_derived_marked(id, idx);
        }
        let at = id.0 as usize;
        self.reactive.evaluating[at] = true;
        let value = self.eval_derived_marked(id, idx);
        self.reactive.evaluating[at] = false;
        if !self.may_store(id, &value) {
            return RuntimeValue::Unit;
        }
        value
    }

    #[inline]
    fn eval_derived_marked(&mut self, id: NodeId, idx: usize) -> RuntimeValue {
        let dynamic = self.reactive.node_dynamic[idx];
        // A static node records nothing, and hides its reads from an outer one.
        self.tracking.push(dynamic.then(Vec::new));

        let ctx = EvalContext {
            graph: &self.graph,
            program: &self.program,
            node_index: &self.node_index,
        };
        let name = &self.graph.nodes[id.0 as usize].name;
        let receiver = if self.live.instances.is_empty() {
            None
        } else {
            self.live
                .node_receiver
                .get(id.0 as usize)
                .copied()
                .flatten()
        };
        // Compiled code reads slot values, which are stale until recomputed;
        // an instance's values run on the VM.
        let native = if self.vm_only || self.reactive.stale_count > 0 || receiver.is_some() {
            None
        } else {
            self.backend.try_eval_derived(name, &ctx)
        };
        let value = match native {
            Some(Ok(v)) => v,
            Some(Err(_)) => {
                self.replay_on_vm();
                self.eval_derived_with(receiver, idx)
            }
            None => self.eval_derived_with(receiver, idx),
        };

        if let Some(Some(tracked)) = self.tracking.pop() {
            self.commit_tracked_deps(id, tracked);
        }
        value
    }

    /// `eval_derived_vm` for an instance's value, or a module's.
    #[inline]
    fn eval_derived_with(&mut self, receiver: Option<u64>, idx: usize) -> RuntimeValue {
        match receiver {
            None => self.eval_derived_vm(idx),
            Some(_) => self.with_receiver(receiver, |this| this.eval_derived_vm(idx)),
        }
    }

    fn eval_derived_vm(&mut self, idx: usize) -> RuntimeValue {
        let code = self.node_code[idx].clone();
        let value = self.eval_expr(&code, &mut Locals::default());
        // HIR allows control flow in match arms only inside functions.
        debug_assert!(
            self.pending_flow.is_none(),
            "control flow escaped a derived value"
        );
        self.pending_flow = None;
        value
    }

    fn commit_tracked_deps(&mut self, id: NodeId, mut new_deps: Vec<NodeId>) {
        if new_deps.is_empty() {
            return;
        }
        new_deps.sort_unstable_by_key(|d| d.0);
        new_deps.dedup();
        self.graph.set_derived_deps(id, new_deps);
    }

    /// Marks `id` current, if it was stale.
    fn clear_stale(&mut self, id: NodeId) {
        let at = id.0 as usize;
        if self.reactive.stale[at] {
            self.reactive.stale[at] = false;
            self.reactive.stale_count -= 1;
        }
    }

    /// Whether a derived value is stale right now.
    pub(super) fn reactive_stale(&self) -> bool {
        self.reactive.stale_count > 0
    }

    /// Whether the innermost value being computed records its reads.
    pub(super) fn is_tracking(&self) -> bool {
        matches!(self.tracking.last(), Some(Some(_)))
    }

    /// Records a read of graph node `id` by the value being computed. A
    /// value that reads itself while being computed is a cycle.
    #[inline]
    pub(super) fn record_dep(&mut self, id: NodeId) {
        if self.reactive.components {
            if self.reactive.evaluating[id.0 as usize] {
                self.raise(flux_value::ArithError::ReactiveCycle);
                return;
            }
            if !self.reactive.unset.is_empty() && self.reactive.unset.contains(&id) {
                self.raise(flux_value::ArithError::ReadBeforeInit);
                return;
            }
        }
        // Duplicates go when the reads are committed (sorted): checking
        // each read against all the others cost a value that reads a
        // thousand instances a million comparisons.
        if let Some(Some(tracked)) = self.tracking.last_mut() {
            if tracked.last() != Some(&id) {
                tracked.push(id);
                if tracked.len() >= 4096 && tracked.len().is_power_of_two() {
                    tracked.sort_unstable_by_key(|d| d.0);
                    tracked.dedup();
                }
            }
        }
    }

    /// Marks everything computed from source `id` as stale.
    fn mark_stale(&mut self, id: NodeId) {
        let mut stack = std::mem::take(&mut self.reactive.dirty_stack);
        stack.extend_from_slice(&self.graph.dependents[id.0 as usize]);
        while let Some(node) = stack.pop() {
            let at = node.0 as usize;
            let derived = self.reactive.node_of[at]
                .is_some_and(|idx| !self.program.nodes[idx].code.is_empty());
            if !derived || self.reactive.stale[at] {
                continue;
            }
            self.reactive.stale[at] = true;
            self.reactive.stale_count += 1;
            stack.extend_from_slice(&self.graph.dependents[at]);
        }
        self.reactive.dirty_stack = stack;
    }

    /// Brings node `id` up to date before it is read, if it is a stale
    /// derived value.
    pub(super) fn refresh(&mut self, id: NodeId) {
        if self.reactive.stale_count > 0 {
            self.ensure_current(id);
        }
    }

    /// Recomputes `id` if it is stale. Its own reads bring its dependencies up
    /// to date first. A changed value is noted for the watches.
    fn ensure_current(&mut self, id: NodeId) {
        let at = id.0 as usize;
        if !self.reactive.stale[at] {
            return;
        }
        self.reactive.stale[at] = false;
        self.reactive.stale_count -= 1;
        let Some(idx) = self.reactive.node_of[at] else {
            return;
        };
        // A chain of instances (a list, a tree) can be read from its far
        // end, each value reading the next: deep down, grow the stack when
        // low (checking it costs a little on every value).
        let value = if self.reactive.components {
            self.reactive.eval_depth += 1;
            let value = if self.reactive.eval_depth > 32 {
                stacker::maybe_grow(64 * 1024, 2 * 1024 * 1024, || self.eval_derived(id, idx))
            } else {
                self.eval_derived(id, idx)
            };
            self.reactive.eval_depth -= 1;
            value
        } else {
            self.eval_derived(id, idx)
        };
        if self.trap.is_some() {
            self.note_derived_failure(id);
            return;
        }
        let slot = &mut self.graph.nodes[at].value;
        if !flux_value::values_equal(slot, &value) {
            self.reactive.changed_nodes.push(id);
        }
        *slot = value;
    }

    /// Recomputes, in dependency order, the stale values that `sources` reach.
    /// Values made stale by writes still pending elsewhere (an outer handler's,
    /// around a nested `emit`) wait for their own flush, or for a read.
    fn recompute_from(&mut self, sources: &[NodeId]) {
        if self.reactive.stale_count == 0 {
            return;
        }
        let mut stack = std::mem::take(&mut self.reactive.dirty_stack);
        let mut reached = std::mem::take(&mut self.reactive.reached);
        for source in sources {
            stack.extend_from_slice(&self.graph.dependents[source.0 as usize]);
            while let Some(id) = stack.pop() {
                let at = id.0 as usize;
                if !reached[at] {
                    reached[at] = true;
                    stack.extend_from_slice(&self.graph.dependents[at]);
                }
            }
        }
        self.reactive.dirty_stack = stack;
        // Recomputing can rewrite `topo_order`, so the order is fixed first.
        let mut order = std::mem::take(&mut self.reactive.dirty_order);
        for id in self.graph.topo() {
            let at = id.0 as usize;
            if reached[at] {
                reached[at] = false;
                if self.reactive.stale[at] {
                    order.push(id);
                }
            }
        }
        self.reactive.reached = reached;
        for &id in &order {
            self.ensure_current(id);
            if self.trap.is_some() {
                break;
            }
        }
        order.clear();
        self.reactive.dirty_order = order;
    }

    pub(super) fn is_source_binding(&self, name: &str) -> bool {
        self.resolve_node(name)
            .is_some_and(|id| matches!(self.graph.nodes[id.0 as usize].kind, NodeKind::Source))
    }

    fn try_run_jit_on(
        &mut self,
        idx: usize,
        body: &[flux_ir::IrStmt],
        locals: &mut Locals,
        values: &[RuntimeValue],
    ) -> bool {
        // Compiled code reads a copy of the values: a stale one would be old.
        // A body without a loop runs on the VM: copying values into slots and
        // back cost more than its code (an event took 20k instructions
        // compiled, 11k interpreted).
        if self.vm_only || self.reactive_stale() || !has_loop(body) {
            return false;
        }
        // Handler slots hold tagged handles until `apply_jit_assignments` reads them.
        let _heap = flux_value::heap_scope();
        let key = &self.reactive.handler_keys[idx];
        let Some((result, slots)) = self.backend.try_run_handler(key, &self.graph, values) else {
            return false;
        };
        if result.is_err() {
            self.replay_on_vm();
            return false;
        }
        self.apply_jit_assignments(body, locals, &slots);
        true
    }

    fn try_run_jit_watch(
        &mut self,
        idx: usize,
        body: &[flux_ir::IrStmt],
        prev: Option<RuntimeValue>,
    ) -> bool {
        if self.vm_only || self.reactive_stale() || !has_loop(body) {
            return false;
        }
        let _heap = flux_value::heap_scope();
        let key = &self.reactive.watch_keys[idx];
        let Some((result, slots)) = self
            .backend
            .try_run_handler(key, &self.graph, prev.as_slice())
        else {
            return false;
        };
        if result.is_err() {
            self.replay_on_vm();
            return false;
        }
        self.apply_jit_assignments(body, &mut Locals::default(), &slots);
        true
    }

    #[cfg(feature = "jit")]
    fn apply_jit_assignments(
        &mut self,
        body: &[flux_ir::IrStmt],
        locals: &mut Locals,
        slots: &[u64],
    ) {
        use flux_ir::IrStmt;
        // Compiled handlers assign sources only at the top level (see
        // `JitEngine::new`), so these are all the targets the run assigned.
        let assigns: Vec<(String, Option<u32>, RuntimeValue)> = body
            .iter()
            .filter_map(|stmt| {
                let IrStmt::Assign { target, slot, .. } = stmt else {
                    return None;
                };
                let tv = self.backend.slot_tagged_in_buffer(target, slots)?;
                Some((target.clone(), *slot, tagged_to_runtime(tv)))
            })
            .collect();
        for (target, slot, val) in assigns {
            match slot {
                Some(slot) => locals.set(slot, val),
                None => self.set_source(&target, val),
            }
        }
    }

    #[cfg(not(feature = "jit"))]
    fn apply_jit_assignments(
        &mut self,
        _body: &[flux_ir::IrStmt],
        _locals: &mut Locals,
        _slots: &[u64],
    ) {
    }

    /// Whether the watch at `at` in `watch_triggers` should run: on its first
    /// run, or when a trigger's value differs by `==` from the one it saw
    /// last. If so, remembers the current values and returns the old value
    /// of the first trigger (its target), or the current one on a first run.
    fn watch_seen_change(&mut self, at: usize) -> Option<RuntimeValue> {
        let (idx, triggers) = self.reactive.watch_triggers[at].clone();
        let receiver = self.reactive.watch_receivers[at];
        // An instance's watch remembers instances by number, so that what it
        // saw does not keep them (or the instance itself) alive. One with
        // `prev` keeps them, to hand them back.
        let by_number = receiver.is_some() && self.program.watches[idx].prev.is_none();
        let mut current = Vec::with_capacity(triggers.len());
        for &id in &triggers {
            self.with_receiver(receiver, |this| this.refresh(id));
            let value = self.graph.get(id);
            current.push(if by_number {
                live::without_handles(value)
            } else {
                value.clone()
            });
        }
        let memory = &mut self.reactive.watch_memory[at];
        let prev = match memory {
            Some(seen) => {
                let same = seen.len() == current.len()
                    && seen
                        .iter()
                        .zip(&current)
                        .all(|(a, b)| flux_value::values_equal(a, b));
                if same {
                    return None;
                }
                seen.first().cloned()
            }
            None => current.first().cloned(),
        };
        *memory = Some(current);
        Some(prev.unwrap_or(RuntimeValue::Unit))
    }

    /// Runs the watches affected by `changed_nodes`, then those affected by
    /// what they wrote, until nothing is left to run. Each watch runs at most
    /// once per call, so a cycle of watches ends. A new instance's watches
    /// run once, whatever changed.
    fn run_watches(&mut self) {
        if self.trap.is_some() {
            return;
        }
        if self.reactive.watch_triggers.is_empty() {
            self.reactive.changed_nodes.clear();
            self.live.first_runs = false;
            return;
        }
        self.in_watch = true;
        // Nothing changed means the first run: every watch runs once.
        let mut run_all = self.reactive.changed_nodes.is_empty() && !self.live.first_runs;
        let mut ran = HashSet::new();
        loop {
            let first_runs = std::mem::take(&mut self.live.first_runs);
            if !run_all && !first_runs && self.reactive.changed_nodes.is_empty() {
                break;
            }
            std::mem::swap(
                &mut self.reactive.changed_nodes,
                &mut self.reactive.changed_spare,
            );
            for id in &self.reactive.changed_spare {
                self.reactive.changed_marks[id.0 as usize] = true;
            }
            let watch_indices: Vec<usize> = (0..self.reactive.watch_triggers.len())
                .filter(|&at| {
                    let (idx, triggers) = &self.reactive.watch_triggers[at];
                    let receiver = self.reactive.watch_receivers[at];
                    let fresh = self.reactive.watch_memory[at].is_none();
                    let in_scope = receiver.is_some()
                        || self
                            .initial_watches
                            .as_ref()
                            .is_none_or(|w| w.contains(idx));
                    !ran.contains(&at)
                        && in_scope
                        && receiver.is_none_or(|r| self.is_alive(r))
                        && ((run_all && receiver.is_none())
                            // A new instance's watch, or (while modules
                            // initialize) one that has not run yet, runs.
                            || (fresh && (receiver.is_some() || self.initial_watches.is_some()))
                            || triggers
                                .iter()
                                .any(|dep| self.reactive.changed_marks[dep.0 as usize]))
                })
                .collect();
            for id in self.reactive.changed_spare.drain(..) {
                self.reactive.changed_marks[id.0 as usize] = false;
            }
            run_all = false;
            if watch_indices.is_empty() {
                if self.live.first_runs {
                    continue;
                }
                break;
            }
            for at in watch_indices {
                let idx = self.reactive.watch_triggers[at].0;
                let receiver = self.reactive.watch_receivers[at];
                // Compared right before running: an earlier watch may have
                // changed a value back.
                let Some(prev) = self.watch_seen_change(at) else {
                    continue;
                };
                ran.insert(at);
                let body = self.reactive.watch_bodies[idx].clone();
                let prev = self.program.watches[idx].prev.as_ref().map(|_| prev);
                if receiver.is_some() || !self.try_run_jit_watch(idx, &body, prev.clone()) {
                    let mut locals = Locals::default();
                    if let Some(prev) = prev {
                        // `prev` is slot 0.
                        locals.set(0, prev);
                    }
                    self.with_receiver(receiver, |this| this.run_stmts(&body, &mut locals));
                    if self.trap.is_some() {
                        self.push_trace_frame("watch".to_string(), None);
                    }
                }
                if self.trap.is_none() {
                    self.flush_propagation();
                }
                if self.trap.is_some() {
                    self.set_trap_context("`watch`", None);
                    self.in_watch = false;
                    return;
                }
            }
        }
        self.in_watch = false;
    }
}
