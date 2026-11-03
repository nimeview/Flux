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
mod stmts;
mod values;

use std::collections::{HashMap, HashSet};

use flux_ir::{CompiledFunction, CompiledProgram, HandlerKind, NodeKind as IrNodeKind};

use crate::backend::{ActiveBackend, EvalContext, ExecBackend};
use crate::error::{ExecutorError, RuntimeError, TraceFrame};
use crate::graph::{NodeId, NodeKind, ReactiveGraph, RuntimeValue};
use crate::vm::validate_program;
use crate::ExecutionMode;

use values::{code_has_lazy_if, eval_const};

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
    /// Runnable watches in graph order, with the nodes that trigger each.
    watch_triggers: Vec<(usize, Vec<NodeId>)>,
    /// Reused while finding what a write dirtied.
    dirty_marks: Vec<bool>,
    dirty_stack: Vec<NodeId>,
    dirty_order: Vec<NodeId>,
}

pub struct Executor {
    pub graph: ReactiveGraph,
    pub program: CompiledProgram,
    /// Shared so a call can run a body without cloning it.
    functions: HashMap<String, std::sync::Arc<CompiledFunction>>,
    /// `program.nodes` index by binding name.
    node_index: HashMap<String, usize>,
    /// Each node's code, shared so a recomputation does not copy it.
    node_code: Vec<std::sync::Arc<[flux_ir::Inst]>>,
    in_watch: bool,
    /// When true, `set_source` records mutations and skips propagation until flush.
    defer_propagation: bool,
    /// Kept apart so it does not spread the fields calls use over more cache lines.
    reactive: Box<ReactiveState>,
    backend: ActiveBackend,
    /// Active while evaluating a derived node with lazy `if` — records graph reads.
    tracking_scope: Option<Vec<NodeId>>,
    /// First runtime error; once set, evaluation unwinds and nothing else runs.
    trap: Option<Box<RuntimeError>>,
    /// Compiled code failed: the VM reruns it, and runs everything from then on.
    vm_only: bool,
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
}

impl Executor {
    /// Create an executor with the default VM engine.
    pub fn new(program: CompiledProgram) -> Result<Self, ExecutorError> {
        Self::with_mode(program, ExecutionMode::Vm)
    }

    /// Create an executor with an explicit engine choice.
    pub fn with_mode(
        mut program: CompiledProgram,
        mode: ExecutionMode,
    ) -> Result<Self, ExecutorError> {
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
        for node in &program.nodes {
            match node.kind {
                IrNodeKind::Source => {
                    let value = eval_const(&node.init).unwrap_or(RuntimeValue::Unit);
                    graph.add_source(&node.name, value);
                }
                IrNodeKind::Derived => {
                    let deps: Vec<_> = node
                        .deps
                        .iter()
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

        let node_index: HashMap<String, usize> = program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| matches!(n.kind, IrNodeKind::Source | IrNodeKind::Derived))
            .map(|(i, n)| (n.name.clone(), i))
            .collect();
        let node_code = program
            .nodes
            .iter()
            .map(|n| std::sync::Arc::from(n.code.as_slice()))
            .collect();

        let node_dynamic = program
            .nodes
            .iter()
            .map(|n| code_has_lazy_if(&n.code))
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
                    NodeKind::Effect { meta } if meta.handler_kind == HandlerKind::On => {
                        Some(meta.handler_index)
                    }
                    _ => None,
                })
                .collect();
            if !handlers.is_empty() {
                event_handlers.insert(name.clone(), handlers.into());
            }
        }
        let watch_triggers = graph
            .nodes
            .iter()
            .filter_map(|node| {
                let NodeKind::Effect { meta } = &node.kind else {
                    return None;
                };
                if meta.handler_kind != HandlerKind::Watch {
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

        let backend = ActiveBackend::from_mode(mode, &program);

        let mut this = Self {
            graph,
            program,
            functions,
            node_index,
            node_code,
            in_watch: false,
            defer_propagation: false,
            reactive: Box::new(ReactiveState {
                mutated_sources: Vec::new(),
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
                dirty_marks: vec![false; node_count],
                dirty_stack: Vec::new(),
                dirty_order: Vec::new(),
            }),
            backend,
            tracking_scope: None,
            stack_pool: Vec::new(),
            locals_pool: Vec::new(),
            pending_flow: None,
            call_depth: 0,
            trap: None,
            vm_only: false,
        };
        // Initial values are assigned without propagation; derived values and
        // watches then run once over the fully initialized graph.
        this.defer_propagation = true;
        for stmt in &this.program.init_stmts.clone() {
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
        this.defer_propagation = false;
        this.reactive.mutated_sources.clear();
        this.reactive.changed_nodes.clear();
        if this.trap.is_none() {
            this.recompute_all();
            this.run_watches();
        }
        if let Some(err) = this.trap.take() {
            return Err(ExecutorError::Runtime(*err));
        }
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
        self.functions.get(name).map(|f| f.params.len())
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
        let value = self.call_function(name, args);
        self.set_trap_context(&format!("`{name}`"), None);
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

    /// Emit an event by name (for tests and host integration).
    pub fn emit(&mut self, event: &str, payload: RuntimeValue) {
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
            self.run_event_handlers(event, payload)
        });
        self.call_depth -= 1;
    }

    fn run_event_handlers(&mut self, event: &str, payload: RuntimeValue) {
        let Some(handlers) = self.reactive.event_handlers.get(event).cloned() else {
            return;
        };

        // The body that emitted is unfinished: its writes stay pending.
        // (Taken only when present: an event must not drop the set's capacity.)
        let outer_writes = (!self.reactive.mutated_sources.is_empty())
            .then(|| std::mem::take(&mut self.reactive.mutated_sources));
        let was_deferred = self.defer_propagation;
        self.defer_propagation = true;
        for &idx in handlers.iter() {
            let body = self.reactive.handler_bodies[idx].clone();
            let mut locals = self.locals_pool.pop().unwrap_or_default();
            if self.program.on_handlers[idx].param.is_some() {
                // A handler's parameter is slot 0.
                locals.set(0, payload.clone());
            }
            if !self.try_run_jit_on(idx, &body, &mut locals, payload.clone()) {
                self.run_stmts(&body, &mut locals);
                if self.trap.is_some() {
                    self.push_trace_frame(format!("on {event}"), None);
                }
            }
            locals.clear();
            if self.locals_pool.len() < LOCALS_POOL_LIMIT {
                self.locals_pool.push(locals);
            }
            if self.trap.is_some() {
                self.set_trap_context(&format!("`on {event}`"), None);
                break;
            }
            self.flush_propagation();
            if self.trap.is_some() {
                break;
            }
        }
        self.defer_propagation = was_deferred;
        if let Some(outer_writes) = outer_writes {
            self.reactive.mutated_sources.extend(outer_writes);
        }
    }

    pub fn set_source(&mut self, name: &str, value: RuntimeValue) {
        if self.trap.is_some() {
            return;
        }
        let Some(&id) = self.graph.name_to_id.get(name) else {
            return;
        };
        if !matches!(self.graph.nodes[id.0 as usize].kind, NodeKind::Source) {
            return;
        }
        self.graph.set_source(id, value);
        // A handler or watch body writes as a batch, applied when it ends.
        if self.defer_propagation || self.in_watch {
            if !self.reactive.mutated_sources.contains(&id) {
                self.reactive.mutated_sources.push(id);
            }
            return;
        }
        self.propagate_source_change(id);
    }

    fn propagate_source_change(&mut self, id: NodeId) {
        self.reactive.changed_nodes.push(id);
        self.recompute_from_sources(&[id]);
        if !self.in_watch {
            self.run_watches();
        }
    }

    fn flush_propagation(&mut self) {
        if self.reactive.mutated_sources.is_empty() {
            return;
        }
        let mut sources = std::mem::take(&mut self.reactive.mutated_sources);
        self.reactive.changed_nodes.extend_from_slice(&sources);
        self.recompute_from_sources(&sources);
        sources.clear();
        self.reactive.mutated_sources = sources;
        if !self.in_watch {
            self.run_watches();
        }
    }

    fn recompute_all(&mut self) {
        let derived: Vec<_> = self
            .graph
            .topo_order
            .iter()
            .filter_map(|&id| {
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
        }
    }

    #[cold]
    fn note_derived_failure(&mut self, id: NodeId) {
        let name = self.graph.nodes[id.0 as usize].name.clone();
        let declared = self.program.source_map.bindings.get(&name).copied();
        self.push_trace_frame(name.clone(), declared);
        self.set_trap_context(&format!("`{name}`"), Some(&name));
    }

    /// Evaluate one derived node via the active backend, falling back to VM bytecode.
    fn eval_derived(&mut self, id: NodeId, idx: usize) -> RuntimeValue {
        let dynamic = self.reactive.node_dynamic[idx];
        if dynamic {
            self.tracking_scope = Some(Vec::new());
        }

        let ctx = EvalContext {
            graph: &self.graph,
            program: &self.program,
            node_index: &self.node_index,
        };
        let name = &self.graph.nodes[id.0 as usize].name;
        let native = if self.vm_only {
            None
        } else {
            self.backend.try_eval_derived(name, &ctx)
        };
        let value = match native {
            Some(Err(_)) => {
                self.replay_on_vm();
                self.eval_derived_vm(idx)
            }
            Some(Ok(v)) => v,
            None => self.eval_derived_vm(idx),
        };

        if dynamic {
            self.commit_tracked_deps(id);
        }
        value
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

    fn commit_tracked_deps(&mut self, id: NodeId) {
        let Some(mut new_deps) = self.tracking_scope.take() else {
            return;
        };
        if new_deps.is_empty() {
            return;
        }
        new_deps.sort_by_key(|d| d.0);
        self.graph.set_derived_deps(id, new_deps);
    }

    /// Records a read of global `name` (a local is never a dependency).
    pub(super) fn record_dep_load(&mut self, name: &str) {
        let Some(tracked) = self.tracking_scope.as_mut() else {
            return;
        };
        if let Some(&id) = self.graph.name_to_id.get(name) {
            if !tracked.contains(&id) {
                tracked.push(id);
            }
        }
    }

    fn recompute_from_sources(&mut self, sources: &[NodeId]) {
        let mut stack = std::mem::take(&mut self.reactive.dirty_stack);
        for source in sources {
            stack.extend_from_slice(&self.graph.dependents[source.0 as usize]);
            while let Some(id) = stack.pop() {
                let mark = &mut self.reactive.dirty_marks[id.0 as usize];
                if !*mark {
                    *mark = true;
                    stack.extend_from_slice(&self.graph.dependents[id.0 as usize]);
                }
            }
        }
        self.reactive.dirty_stack = stack;

        // Recomputing can rewrite `topo_order`, so the order is fixed first.
        let mut order = std::mem::take(&mut self.reactive.dirty_order);
        for &id in &self.graph.topo_order {
            let mark = &mut self.reactive.dirty_marks[id.0 as usize];
            if *mark {
                *mark = false;
                order.push(id);
            }
        }

        for &id in &order {
            let Some(idx) = self.reactive.node_of[id.0 as usize] else {
                continue;
            };
            if self.program.nodes[idx].code.is_empty() {
                continue;
            }
            let value = self.eval_derived(id, idx);
            if self.trap.is_some() {
                self.note_derived_failure(id);
                break;
            }
            let slot = &mut self.graph.nodes[id.0 as usize].value;
            if *slot != value {
                self.reactive.changed_nodes.push(id);
            }
            *slot = value;
        }
        order.clear();
        self.reactive.dirty_order = order;
    }

    pub(super) fn is_source_binding(&self, name: &str) -> bool {
        self.graph
            .name_to_id
            .get(name)
            .is_some_and(|&id| matches!(self.graph.nodes[id.0 as usize].kind, NodeKind::Source))
    }

    fn try_run_jit_on(
        &mut self,
        idx: usize,
        body: &[flux_ir::IrStmt],
        locals: &mut Locals,
        param: RuntimeValue,
    ) -> bool {
        if self.vm_only {
            return false;
        }
        // Handler slots hold tagged handles until `apply_jit_assignments` reads them.
        let _heap = flux_value::heap_scope();
        let handler = &self.program.on_handlers[idx];
        let param_ref = handler.param.as_ref().map(|_| &param);
        let key = &self.reactive.handler_keys[idx];
        let Some((result, slots)) = self.backend.try_run_handler(key, &self.graph, param_ref)
        else {
            return false;
        };
        if result.is_err() {
            self.replay_on_vm();
            return false;
        }
        self.apply_jit_assignments(body, locals, &slots);
        true
    }

    fn try_run_jit_watch(&mut self, idx: usize, body: &[flux_ir::IrStmt]) -> bool {
        if self.vm_only {
            return false;
        }
        let _heap = flux_value::heap_scope();
        let key = &self.reactive.watch_keys[idx];
        let Some((result, slots)) = self.backend.try_run_handler(key, &self.graph, None) else {
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

    /// Runs the watches affected by `changed_nodes`, then those affected by
    /// what they wrote, until nothing is left to run. Each watch runs at most
    /// once per call, so a cycle of watches ends.
    fn run_watches(&mut self) {
        if self.trap.is_some() {
            return;
        }
        if self.reactive.watch_triggers.is_empty() {
            self.reactive.changed_nodes.clear();
            return;
        }
        self.in_watch = true;
        // Nothing changed means the first run: every watch runs once.
        let mut run_all = self.reactive.changed_nodes.is_empty();
        let mut ran = HashSet::new();
        loop {
            if !run_all && self.reactive.changed_nodes.is_empty() {
                break;
            }
            std::mem::swap(
                &mut self.reactive.changed_nodes,
                &mut self.reactive.changed_spare,
            );
            for id in &self.reactive.changed_spare {
                self.reactive.changed_marks[id.0 as usize] = true;
            }
            let watch_indices: Vec<usize> = self
                .reactive
                .watch_triggers
                .iter()
                .filter(|(idx, triggers)| {
                    !ran.contains(idx)
                        && (run_all
                            || triggers
                                .iter()
                                .any(|dep| self.reactive.changed_marks[dep.0 as usize]))
                })
                .map(|(idx, _)| *idx)
                .collect();
            for id in self.reactive.changed_spare.drain(..) {
                self.reactive.changed_marks[id.0 as usize] = false;
            }
            run_all = false;
            if watch_indices.is_empty() {
                break;
            }
            for idx in watch_indices {
                ran.insert(idx);
                let body = self.reactive.watch_bodies[idx].clone();
                if !self.try_run_jit_watch(idx, &body) {
                    self.run_stmts(&body, &mut Locals::default());
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
