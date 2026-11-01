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
    /// Slot buffer reused by calls and derived values, which are frequent
    /// (one per derived node per event); a re-entrant use gets a fresh one.
    buffer: std::cell::RefCell<Vec<u64>>,
}

#[cfg(feature = "jit")]
impl JitBackend {
    pub fn new(program: &CompiledProgram) -> Self {
        Self {
            engine: flux_jit::get_or_compile(program),
            buffer: std::cell::RefCell::new(Vec::new()),
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

    /// `depth`: calls already active, so nested JIT calls count from there.
    pub fn try_call_fn(
        &self,
        name: &str,
        args: &[RuntimeValue],
        graph: &ReactiveGraph,
        depth: u64,
    ) -> Option<JitResult> {
        // A struct, `Option`, or `Result` has no tagged form (it would arrive
        // as `()`): the VM runs such a call.
        if !args.iter().all(flux_value::is_tag_representable) {
            return None;
        }
        let _heap = flux_value::heap_scope();
        self.with_slots(graph, name, |slots| {
            slots[self.engine.depth_pair() as usize * 2 + 1] = depth;
            for (i, arg) in args.iter().enumerate() {
                let tv = flux_value::value_to_tagged(arg);
                let off = (self.engine.host_slot_count() as usize + i) * 2;
                slots[off] = tv.tag;
                slots[off + 1] = tv.data;
            }
            self.engine.call_fn_with_slots(name, slots).map(from_tagged)
        })?
    }

    pub fn try_run_handler(
        &self,
        key: &str,
        graph: &ReactiveGraph,
        param: Option<&RuntimeValue>,
    ) -> Option<HandlerResult> {
        if !param.is_none_or(flux_value::is_tag_representable) {
            return None;
        }
        // The returned slots hold tagged handles: the caller keeps a
        // `flux_value::heap_scope` open until it has read them.
        let mut slots = Vec::new();
        if !fill_slots(&self.engine, graph, key, &mut slots) {
            return None;
        }
        let param_tv = param.map(flux_value::value_to_tagged);
        slots = self.engine.build_stmt_slots(key, &slots, param_tv);
        flux_jit::take_emit_events();
        let result = self.engine.run_stmts_with(key, &slots).map(from_tagged)?;
        let emitted = flux_jit::take_emit_events();
        if result.is_err() {
            // The handler aborted; its queued emits and slot writes are discarded.
            return Some((result, Vec::new(), slots));
        }
        if emitted.is_empty() {
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
        self.with_slots(graph, name, |slots| {
            self.engine.eval_expr_with(name, slots).map(from_tagged)
        })?
    }

    /// Runs `f` on the reused slot buffer filled for entry `key` (a fresh
    /// one if it is in use); `None` when an input has no tagged form.
    fn with_slots<R>(
        &self,
        graph: &ReactiveGraph,
        key: &str,
        f: impl FnOnce(&mut Vec<u64>) -> R,
    ) -> Option<R> {
        let mut fresh = Vec::new();
        let mut shared = self.buffer.try_borrow_mut();
        let slots = match shared.as_mut() {
            Ok(buffer) => &mut **buffer,
            Err(_) => &mut fresh,
        };
        fill_slots(&self.engine, graph, key, slots).then(|| f(slots))
    }
}

#[cfg(feature = "jit")]
/// Fills `slots` for compiled entry `key`; `false` when a node it reads has
/// no tagged form (a struct, `Option`, or `Result` would arrive as `()`), and
/// the VM runs the entry instead.
fn fill_slots(
    engine: &flux_jit::JitEngine,
    graph: &ReactiveGraph,
    key: &str,
    slots: &mut Vec<u64>,
) -> bool {
    slots.clear();
    slots.extend_from_slice(&engine.layout.flat);
    slots.resize(engine.slot_capacity_pairs() as usize * 2, 0);
    // Only the nodes this entry reads or assigns: converting every node for
    // each event made a compiled handler slower than the VM.
    for name in engine.reads(key) {
        let (Some(&idx), Some(value)) =
            (engine.layout.name_to_idx.get(name), graph.get_by_name(name))
        else {
            continue;
        };
        if !flux_value::is_tag_representable(value) {
            return false;
        }
        let tv = flux_value::value_to_tagged(value);
        let base = idx as usize * 2;
        if base + 1 < slots.len() {
            slots[base] = tv.tag;
            slots[base + 1] = tv.data;
        }
    }
    true
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
        depth: u64,
    ) -> Option<JitResult> {
        match self {
            Self::Jit(b) => b.try_call_fn(name, args, graph, depth),
            Self::Vm(_) => None,
        }
    }

    #[cfg(not(feature = "jit"))]
    pub fn try_call_fn(
        &self,
        _name: &str,
        _args: &[RuntimeValue],
        _graph: &ReactiveGraph,
        _depth: u64,
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
