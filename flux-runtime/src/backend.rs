use std::collections::HashMap;
#[cfg(feature = "jit")]
use std::rc::Rc;

use flux_ir::CompiledProgram;

use crate::graph::{ReactiveGraph, RuntimeValue};
use crate::ExecutionMode;

/// A JIT-computed value, or the runtime error the compiled code raised.
pub type JitResult = Result<RuntimeValue, flux_value::ArithError>;

type HandlerResult = (JitResult, Vec<(String, RuntimeValue)>, Vec<u64>);

#[cfg(feature = "jit")]
fn from_tagged(tv: flux_value::TaggedVal) -> JitResult {
    if tv.is_error() {
        Err(flux_value::ArithError::from_code(tv.data))
    } else {
        Ok(flux_value::tagged_to_value(tv))
    }
}

/// Read-only context shared while evaluating derived nodes.
pub struct EvalContext<'a> {
    pub graph: &'a ReactiveGraph,
    pub program: &'a CompiledProgram,
    pub node_index: &'a HashMap<String, usize>,
}

/// Strategy for evaluating derived reactive nodes (VM bytecode vs JIT).
pub trait ExecBackend {
    fn mode(&self) -> ExecutionMode;

    fn try_eval_derived(&mut self, name: &str, ctx: &EvalContext<'_>) -> Option<JitResult>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct VmBackend;

impl ExecBackend for VmBackend {
    fn mode(&self) -> ExecutionMode {
        ExecutionMode::Vm
    }

    fn try_eval_derived(&mut self, _name: &str, _ctx: &EvalContext<'_>) -> Option<JitResult> {
        None
    }
}

#[cfg(feature = "jit")]
pub struct JitBackend {
    pub(crate) engine: Rc<flux_jit::JitEngine>,
}

#[cfg(feature = "jit")]
impl JitBackend {
    pub fn new(program: &CompiledProgram) -> Self {
        Self {
            engine: flux_jit::get_or_compile(program),
        }
    }

    pub fn mode(&self) -> ExecutionMode {
        ExecutionMode::Jit
    }

    pub fn stats(&self) -> flux_jit::JitStats {
        self.engine.stats()
    }

    pub fn lazy_if_deps_for(&self, name: &str) -> Option<std::collections::HashSet<String>> {
        self.engine.lazy_if_deps_for(name).cloned()
    }

    pub fn try_call_fn(
        &self,
        name: &str,
        args: &[RuntimeValue],
        graph: &ReactiveGraph,
    ) -> Option<JitResult> {
        let _heap = flux_value::heap_scope();
        let mut slots = build_slot_buffer(&self.engine, graph);
        for (i, arg) in args.iter().enumerate() {
            let tv = flux_value::value_to_tagged(arg);
            let off = (self.engine.host_slot_count() as usize + i) * 2;
            slots[off] = tv.tag;
            slots[off + 1] = tv.data;
        }
        self.engine
            .call_fn_with_slots(name, &slots)
            .map(from_tagged)
    }

    pub fn try_run_handler(
        &self,
        key: &str,
        graph: &ReactiveGraph,
        param: Option<&RuntimeValue>,
    ) -> Option<HandlerResult> {
        // The returned slots hold tagged handles: the caller keeps a
        // `flux_value::heap_scope` open until it has read them.
        let mut slots = build_slot_buffer(&self.engine, graph);
        let param_tv = param.map(flux_value::value_to_tagged);
        slots = self.engine.build_stmt_slots(key, &slots, param_tv);
        flux_jit::take_emit_events();
        let result = self.engine.run_stmts_with(key, &slots).map(from_tagged)?;
        let emitted = flux_jit::take_emit_events();
        if result.is_err() {
            // The handler aborted; its queued emits and slot writes are discarded.
            return Some((result, Vec::new(), slots));
        }
        let event_table: HashMap<u64, String> = self
            .engine
            .event_indices
            .iter()
            .map(|(name, idx)| (*idx as u64, name.clone()))
            .collect();
        let emits = emitted
            .into_iter()
            .filter_map(|(idx, tv)| {
                event_table
                    .get(&idx)
                    .map(|event| (event.clone(), flux_value::tagged_to_value(tv)))
            })
            .collect();
        Some((result, emits, slots))
    }

    pub fn slot_tagged_in_buffer(
        &self,
        name: &str,
        slots: &[u64],
    ) -> Option<flux_value::TaggedVal> {
        let idx = *self.engine.layout.name_to_idx.get(name)? as usize * 2;
        Some(flux_value::TaggedVal {
            tag: *slots.get(idx)?,
            data: *slots.get(idx + 1)?,
        })
    }

    pub fn try_eval_derived(&self, name: &str, graph: &ReactiveGraph) -> Option<JitResult> {
        if self.engine.vm_fallback.contains(name) {
            return None;
        }
        let _heap = flux_value::heap_scope();
        let slots = build_slot_buffer(&self.engine, graph);
        self.engine.eval_expr_with(name, &slots).map(from_tagged)
    }
}

#[cfg(feature = "jit")]
fn build_slot_buffer(engine: &flux_jit::JitEngine, graph: &ReactiveGraph) -> Vec<u64> {
    let mut flat = engine.layout.flat.clone();
    flat.resize(engine.slot_capacity_pairs() as usize * 2, 0);
    for (name, &id) in &graph.name_to_id {
        let Some(&idx) = engine.layout.name_to_idx.get(name) else {
            continue;
        };
        let tv = flux_value::value_to_tagged(&graph.nodes[id.0 as usize].value);
        let base = idx as usize * 2;
        if base + 1 < flat.len() {
            flat[base] = tv.tag;
            flat[base + 1] = tv.data;
        }
    }
    flat
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum ActiveBackend {
    Vm(VmBackend),
    #[cfg(feature = "jit")]
    Jit(JitBackend),
}

impl ActiveBackend {
    pub fn from_mode(mode: ExecutionMode, program: &CompiledProgram) -> Self {
        #[cfg(not(feature = "jit"))]
        let _ = program;
        match mode {
            ExecutionMode::Vm => Self::Vm(VmBackend),
            #[cfg(feature = "jit")]
            ExecutionMode::Jit => Self::Jit(JitBackend::new(program)),
            #[cfg(not(feature = "jit"))]
            ExecutionMode::Jit => Self::Vm(VmBackend),
        }
    }

    pub fn mode(&self) -> ExecutionMode {
        match self {
            Self::Vm(b) => b.mode(),
            #[cfg(feature = "jit")]
            Self::Jit(b) => b.mode(),
        }
    }

    #[cfg(feature = "jit")]
    pub fn jit_stats(&self) -> Option<flux_jit::JitStats> {
        match self {
            Self::Jit(b) => Some(b.stats()),
            Self::Vm(_) => None,
        }
    }

    #[cfg(not(feature = "jit"))]
    #[allow(dead_code)]
    pub fn jit_stats(&self) -> Option<()> {
        None
    }

    #[cfg(feature = "jit")]
    pub fn lazy_if_deps_for(&self, name: &str) -> Option<std::collections::HashSet<String>> {
        match self {
            Self::Jit(b) => b.lazy_if_deps_for(name),
            Self::Vm(_) => None,
        }
    }

    #[cfg(not(feature = "jit"))]
    pub fn lazy_if_deps_for(&self, _name: &str) -> Option<std::collections::HashSet<String>> {
        None
    }

    #[cfg(feature = "jit")]
    pub fn try_call_fn(
        &self,
        name: &str,
        args: &[RuntimeValue],
        graph: &ReactiveGraph,
    ) -> Option<JitResult> {
        match self {
            Self::Jit(b) => b.try_call_fn(name, args, graph),
            Self::Vm(_) => None,
        }
    }

    #[cfg(not(feature = "jit"))]
    pub fn try_call_fn(
        &self,
        _name: &str,
        _args: &[RuntimeValue],
        _graph: &ReactiveGraph,
    ) -> Option<JitResult> {
        None
    }

    pub fn try_run_handler(
        &mut self,
        key: &str,
        graph: &ReactiveGraph,
        param: Option<&RuntimeValue>,
    ) -> Option<HandlerResult> {
        #[cfg(feature = "jit")]
        {
            if let Self::Jit(b) = self {
                return b.try_run_handler(key, graph, param);
            }
        }
        let _ = (key, graph, param);
        None
    }

    #[cfg(feature = "jit")]
    pub fn slot_tagged_in_buffer(
        &self,
        name: &str,
        slots: &[u64],
    ) -> Option<flux_value::TaggedVal> {
        match self {
            Self::Jit(b) => b.slot_tagged_in_buffer(name, slots),
            Self::Vm(_) => None,
        }
    }

    #[cfg(not(feature = "jit"))]
    #[allow(dead_code)]
    pub fn slot_tagged_in_buffer(&self, _name: &str, _slots: &[u64]) -> Option<()> {
        None
    }
}

impl ExecBackend for ActiveBackend {
    fn mode(&self) -> ExecutionMode {
        match self {
            Self::Vm(b) => b.mode(),
            #[cfg(feature = "jit")]
            Self::Jit(b) => b.mode(),
        }
    }

    fn try_eval_derived(&mut self, name: &str, ctx: &EvalContext<'_>) -> Option<JitResult> {
        match self {
            Self::Vm(b) => b.try_eval_derived(name, ctx),
            #[cfg(feature = "jit")]
            Self::Jit(b) => b.try_eval_derived(name, ctx.graph),
        }
    }
}
