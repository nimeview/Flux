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
use crate::error::{ExecutorError, RuntimeError};
use crate::graph::{NodeKind, ReactiveGraph, RuntimeValue};
use crate::vm::validate_program;
use crate::ExecutionMode;

use values::{code_has_lazy_if, eval_const, watch_should_run};

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

pub struct Executor {
    pub graph: ReactiveGraph,
    pub program: CompiledProgram,
    functions: HashMap<String, CompiledFunction>,
    /// `program.nodes` index by binding name.
    node_index: HashMap<String, usize>,
    in_watch: bool,
    /// When true, `set_source` records mutations and skips propagation until flush.
    defer_propagation: bool,
    mutated_sources: HashSet<String>,
    changed_nodes: HashSet<String>,
    backend: ActiveBackend,
    /// Active while evaluating a derived node with lazy `if` — records graph reads.
    tracking_scope: Option<HashSet<crate::graph::NodeId>>,
    /// First runtime error; once set, evaluation unwinds and nothing else runs.
    trap: Option<RuntimeError>,
}

impl Executor {
    /// Create an executor with the default VM engine.
    pub fn new(program: CompiledProgram) -> Result<Self, ExecutorError> {
        Self::with_mode(program, ExecutionMode::Vm)
    }

    /// Create an executor with an explicit engine choice.
    pub fn with_mode(program: CompiledProgram, mode: ExecutionMode) -> Result<Self, ExecutorError> {
        validate_program(&program).map_err(ExecutorError::Invalid)?;

        let functions = program
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.clone()))
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

        let backend = ActiveBackend::from_mode(mode, &program);

        let mut this = Self {
            graph,
            program,
            functions,
            node_index,
            in_watch: false,
            defer_propagation: false,
            mutated_sources: HashSet::new(),
            changed_nodes: HashSet::new(),
            backend,
            tracking_scope: None,
            trap: None,
        };
        // Initial values are assigned without propagation; derived values and
        // watches then run once over the fully initialized graph.
        this.defer_propagation = true;
        for stmt in &this.program.init_stmts.clone() {
            this.run_stmt(stmt, &mut HashMap::new());
            if this.trap.is_some() {
                let binding = match stmt {
                    flux_ir::IrStmt::Assign { target, .. } => Some(target.as_str()),
                    _ => None,
                };
                this.set_trap_context("module initialization", binding);
                break;
            }
        }
        this.defer_propagation = false;
        this.mutated_sources.clear();
        this.changed_nodes.clear();
        this.recompute_all();
        this.run_watches();
        if let Some(err) = this.trap.take() {
            return Err(ExecutorError::Runtime(err));
        }
        Ok(this)
    }

    /// The first runtime error raised, if any. Once set, the executor stops
    /// running code; values computed before the error stay readable.
    pub fn runtime_error(&self) -> Option<&RuntimeError> {
        self.trap.as_ref()
    }

    pub(crate) fn raise(&mut self, kind: flux_value::ArithError) {
        if self.trap.is_none() {
            self.trap = Some(RuntimeError {
                kind,
                context: None,
                binding: None,
            });
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
        let Some(&event_id) = self.graph.name_to_id.get(event) else {
            return;
        };
        let handler_indices: Vec<usize> = self
            .graph
            .effects_for_event(event_id)
            .iter()
            .filter_map(|&effect_id| {
                let NodeKind::Effect { meta } = &self.graph.nodes[effect_id.0 as usize].kind else {
                    return None;
                };
                if meta.handler_kind != HandlerKind::On {
                    return None;
                }
                Some(meta.handler_index)
            })
            .collect();

        let was_deferred = self.defer_propagation;
        self.defer_propagation = true;
        for idx in handler_indices {
            let handler = self.program.on_handlers[idx].clone();
            let mut locals = HashMap::new();
            if let Some(param) = &handler.param {
                locals.insert(param.clone(), payload.clone());
            }
            if !self.try_run_jit_on(idx, &handler.body, &mut locals, payload.clone()) {
                self.run_stmts(&handler.body, &mut locals);
            }
            if self.trap.is_some() {
                self.set_trap_context(&format!("`on {event}`"), None);
                break;
            }
            self.flush_propagation();
        }
        self.defer_propagation = was_deferred;
    }

    pub fn set_source(&mut self, name: &str, value: RuntimeValue) {
        if self.trap.is_some() {
            return;
        }
        if !self.graph.set_by_name(name, value) {
            return;
        }
        if self.defer_propagation {
            self.mutated_sources.insert(name.to_string());
            return;
        }
        self.propagate_source_change(name);
    }

    fn propagate_source_change(&mut self, name: &str) {
        self.changed_nodes.insert(name.to_string());
        self.recompute_from_sources(std::iter::once(name));
        if !self.in_watch {
            self.run_watches();
        }
    }

    fn flush_propagation(&mut self) {
        if self.mutated_sources.is_empty() {
            return;
        }
        let sources: Vec<String> = self.mutated_sources.drain().collect();
        for name in &sources {
            self.changed_nodes.insert(name.clone());
        }
        self.recompute_from_sources(sources.iter().map(String::as_str));
        if !self.in_watch {
            self.run_watches();
        }
    }

    fn recompute_all(&mut self) {
        let derived: Vec<_> = self
            .graph
            .topo_order
            .iter()
            .filter_map(|id| {
                let node = &self.graph.nodes[id.0 as usize];
                matches!(node.kind, NodeKind::Derived { .. }).then(|| node.name.clone())
            })
            .collect();
        for name in derived {
            let value = self.eval_derived(&name);
            if self.trap.is_some() {
                self.set_trap_context(&format!("`{name}`"), Some(&name));
                return;
            }
            if let Some(id) = self.graph.name_to_id.get(&name).copied() {
                self.graph.nodes[id.0 as usize].value = value;
            }
        }
    }

    /// Evaluate one derived node via the active backend, falling back to VM bytecode.
    fn eval_derived(&mut self, name: &str) -> RuntimeValue {
        let dynamic = self.derived_uses_lazy_if(name);
        if dynamic {
            self.tracking_scope = Some(HashSet::new());
        }

        let ctx = EvalContext {
            graph: &self.graph,
            program: &self.program,
            node_index: &self.node_index,
        };
        let jit_value = self.backend.try_eval_derived(name, &ctx);
        let value = match jit_value {
            Some(Err(e)) => {
                self.raise(e);
                RuntimeValue::Unit
            }
            Some(Ok(v)) => {
                if dynamic {
                    if let Some(deps) = self.backend.lazy_if_deps_for(name) {
                        if let Some(scope) = &mut self.tracking_scope {
                            for dep_name in deps {
                                if let Some(&id) = self.graph.name_to_id.get(&dep_name) {
                                    scope.insert(id);
                                }
                            }
                        }
                    } else {
                        let _ = self.eval_derived_vm(name);
                    }
                }
                v
            }
            None => self.eval_derived_vm(name),
        };

        if dynamic {
            self.commit_tracked_deps(name);
        }
        value
    }

    fn eval_derived_vm(&mut self, name: &str) -> RuntimeValue {
        let code = self
            .node_index
            .get(name)
            .map(|&idx| self.program.nodes[idx].code.clone())
            .unwrap_or_default();
        self.eval_expr(&code, &mut HashMap::new())
    }

    fn derived_uses_lazy_if(&self, name: &str) -> bool {
        self.node_index
            .get(name)
            .map(|&idx| code_has_lazy_if(&self.program.nodes[idx].code))
            .unwrap_or(false)
    }

    fn commit_tracked_deps(&mut self, name: &str) {
        let Some(tracked) = self.tracking_scope.take() else {
            return;
        };
        if tracked.is_empty() {
            return;
        }
        let Some(&id) = self.graph.name_to_id.get(name) else {
            return;
        };
        let mut new_deps: Vec<_> = tracked.into_iter().collect();
        new_deps.sort_by_key(|d| d.0);
        self.graph.set_derived_deps(id, new_deps);
    }

    pub(super) fn record_dep_load(&mut self, name: &str, locals: &HashMap<String, RuntimeValue>) {
        if locals.contains_key(name) {
            return;
        }
        let Some(tracked) = self.tracking_scope.as_mut() else {
            return;
        };
        if let Some(&id) = self.graph.name_to_id.get(name) {
            tracked.insert(id);
        }
    }

    fn recompute_from_sources<'a, I>(&mut self, sources: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        use std::collections::VecDeque;

        let mut dirty = HashSet::new();
        for source_name in sources {
            let Some(&source_id) = self.graph.name_to_id.get(source_name) else {
                continue;
            };
            let mut queue = VecDeque::new();
            for &dep in &self.graph.dependents[source_id.0 as usize] {
                queue.push_back(dep);
            }
            while let Some(id) = queue.pop_front() {
                if !dirty.insert(id) {
                    continue;
                }
                for &next in &self.graph.dependents[id.0 as usize] {
                    queue.push_back(next);
                }
            }
        }

        let dirty_ordered: Vec<_> = self
            .graph
            .topo_order
            .iter()
            .filter(|id| dirty.contains(id))
            .copied()
            .collect();

        for id in dirty_ordered {
            let name = self.graph.nodes[id.0 as usize].name.clone();
            if !self.node_has_derived_code(&name) {
                continue;
            }
            let old = self.graph.nodes[id.0 as usize].value.clone();
            let value = self.eval_derived(&name);
            if self.trap.is_some() {
                self.set_trap_context(&format!("`{name}`"), Some(&name));
                return;
            }
            if old != value {
                self.changed_nodes.insert(name.clone());
            }
            self.graph.nodes[id.0 as usize].value = value;
        }
    }

    fn node_has_derived_code(&self, name: &str) -> bool {
        self.node_index
            .get(name)
            .map(|&idx| !self.program.nodes[idx].code.is_empty())
            .unwrap_or(false)
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
        locals: &mut HashMap<String, RuntimeValue>,
        param: RuntimeValue,
    ) -> bool {
        // Handler slots hold tagged handles until `apply_jit_assignments` reads them.
        let _heap = flux_value::heap_scope();
        let key = format!("__on_{idx}");
        let handler = &self.program.on_handlers[idx];
        let param_ref = handler.param.as_ref().map(|_| &param);
        let Some((result, emits, slots)) =
            self.backend.try_run_handler(&key, &self.graph, param_ref)
        else {
            return false;
        };
        if let Err(e) = result {
            self.raise(e);
            return true;
        }
        for (event, payload) in emits {
            self.emit(&event, payload);
        }
        self.apply_jit_assignments(body, locals, &slots);
        true
    }

    fn try_run_jit_watch(&mut self, idx: usize, body: &[flux_ir::IrStmt]) -> bool {
        let _heap = flux_value::heap_scope();
        let key = format!("__watch_{idx}");
        let Some((result, emits, slots)) = self.backend.try_run_handler(&key, &self.graph, None)
        else {
            return false;
        };
        if let Err(e) = result {
            self.raise(e);
            return true;
        }
        for (event, payload) in emits {
            self.emit(&event, payload);
        }
        self.apply_jit_assignments(body, &mut HashMap::new(), &slots);
        true
    }

    #[cfg(feature = "jit")]
    fn apply_jit_assignments(
        &mut self,
        body: &[flux_ir::IrStmt],
        locals: &mut HashMap<String, RuntimeValue>,
        slots: &[u64],
    ) {
        use flux_ir::IrStmt;
        let assigns: Vec<(String, RuntimeValue)> = body
            .iter()
            .filter_map(|stmt| {
                let IrStmt::Assign { target, .. } = stmt else {
                    return None;
                };
                let tv = self.backend.slot_tagged_in_buffer(target, slots)?;
                Some((target.clone(), tagged_to_runtime(tv)))
            })
            .collect();
        for (target, val) in assigns {
            if self.is_source_binding(&target) {
                self.set_source(&target, val);
            } else {
                locals.insert(target, val);
            }
        }
    }

    #[cfg(not(feature = "jit"))]
    fn apply_jit_assignments(
        &mut self,
        _body: &[flux_ir::IrStmt],
        _locals: &mut HashMap<String, RuntimeValue>,
        _slots: &[u64],
    ) {
    }

    fn run_watches(&mut self) {
        if self.trap.is_some() {
            return;
        }
        self.in_watch = true;
        let changed = std::mem::take(&mut self.changed_nodes);
        let run_all = changed.is_empty();

        let watch_indices: Vec<usize> = self
            .graph
            .nodes
            .iter()
            .filter_map(|node| {
                let NodeKind::Effect { meta } = &node.kind else {
                    return None;
                };
                if meta.handler_kind != HandlerKind::Watch {
                    return None;
                }
                let watch = &self.program.watches[meta.handler_index];
                if !run_all && !watch_should_run(watch, &changed) {
                    return None;
                }
                if let Some(target) = &watch.target {
                    self.graph.get_by_name(target)?;
                }
                Some(meta.handler_index)
            })
            .collect();

        for idx in watch_indices {
            let body = self.program.watches[idx].body.clone();
            if !self.try_run_jit_watch(idx, &body) {
                self.run_stmts(&body, &mut HashMap::new());
            }
            if self.trap.is_some() {
                self.set_trap_context("`watch`", None);
                break;
            }
        }
        self.in_watch = false;
    }
}
