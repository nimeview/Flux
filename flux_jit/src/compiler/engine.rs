use std::collections::{HashMap, HashSet};

use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};

use flux_ir::{CompiledProgram, Inst, IrStmt};

use flux_value::TaggedVal;

use super::lower_inst::lower_insts;
use super::lower_stmt::lower_stmts;
use super::numeric::{
    func_is_numeric, is_numeric_code, max_call_argc_in_code, max_call_argc_in_stmts, stmts_numeric,
};
use super::slots::SlotLayout;
use super::tables::JitTables;

/// Imported host helpers used while lowering IR → Cranelift.
pub(crate) struct HostFuncs {
    pub array_push: FuncId,
    pub array_finish: FuncId,
    pub array_len: FuncId,
    pub array_get: FuncId,
    pub range: FuncId,
    pub emit: FuncId,
    pub string_concat: FuncId,
    pub value_eq: FuncId,
    pub make_enum: FuncId,
    pub enum_variant_eq: FuncId,
    pub enum_payload_get: FuncId,
    pub call_builtin: FuncId,
    pub cast: FuncId,
}

impl HostFuncs {
    pub(crate) fn declare(module: &mut JITModule) -> Self {
        let mut push_sig = module.make_signature();
        push_sig.params.push(AbiParam::new(types::I64));
        push_sig.params.push(AbiParam::new(types::I64));
        let array_push = module
            .declare_function("flux_jit_array_push", Linkage::Import, &push_sig)
            .expect("declare flux_jit_array_push");

        let mut finish_sig = module.make_signature();
        finish_sig.params.push(AbiParam::new(types::I64));
        finish_sig.returns.push(AbiParam::new(types::I64));
        finish_sig.returns.push(AbiParam::new(types::I64));
        let array_finish = module
            .declare_function("flux_jit_array_finish", Linkage::Import, &finish_sig)
            .expect("declare flux_jit_array_finish");

        let mut len_sig = module.make_signature();
        len_sig.params.push(AbiParam::new(types::I64));
        len_sig.returns.push(AbiParam::new(types::I64));
        let array_len = module
            .declare_function("flux_jit_array_len", Linkage::Import, &len_sig)
            .expect("declare flux_jit_array_len");

        let mut get_sig = module.make_signature();
        get_sig.params.push(AbiParam::new(types::I64));
        get_sig.params.push(AbiParam::new(types::I64));
        get_sig.returns.push(AbiParam::new(types::I64));
        get_sig.returns.push(AbiParam::new(types::I64));
        let array_get = module
            .declare_function("flux_jit_array_get", Linkage::Import, &get_sig)
            .expect("declare flux_jit_array_get");

        let mut range_sig = module.make_signature();
        for _ in 0..4 {
            range_sig.params.push(AbiParam::new(types::I64));
        }
        range_sig.returns.push(AbiParam::new(types::I64));
        range_sig.returns.push(AbiParam::new(types::I64));
        let range = module
            .declare_function("flux_jit_range", Linkage::Import, &range_sig)
            .expect("declare flux_jit_range");

        let mut emit_sig = module.make_signature();
        emit_sig.params.push(AbiParam::new(types::I64));
        emit_sig.params.push(AbiParam::new(types::I64));
        emit_sig.params.push(AbiParam::new(types::I64));
        let emit = module
            .declare_function("flux_jit_emit", Linkage::Import, &emit_sig)
            .expect("declare flux_jit_emit");

        let mut concat_sig = module.make_signature();
        for _ in 0..4 {
            concat_sig.params.push(AbiParam::new(types::I64));
        }
        concat_sig.returns.push(AbiParam::new(types::I64));
        concat_sig.returns.push(AbiParam::new(types::I64));
        let string_concat = module
            .declare_function("flux_jit_string_concat", Linkage::Import, &concat_sig)
            .expect("declare flux_jit_string_concat");

        let mut eq_sig = module.make_signature();
        for _ in 0..4 {
            eq_sig.params.push(AbiParam::new(types::I64));
        }
        eq_sig.returns.push(AbiParam::new(types::I64));
        let value_eq = module
            .declare_function("flux_jit_value_eq", Linkage::Import, &eq_sig)
            .expect("declare flux_jit_value_eq");

        let mut make_enum_sig = module.make_signature();
        for _ in 0..5 {
            make_enum_sig.params.push(AbiParam::new(types::I64));
        }
        make_enum_sig.returns.push(AbiParam::new(types::I64));
        make_enum_sig.returns.push(AbiParam::new(types::I64));
        let make_enum = module
            .declare_function("flux_jit_make_enum", Linkage::Import, &make_enum_sig)
            .expect("declare flux_jit_make_enum");

        let mut variant_eq_sig = module.make_signature();
        variant_eq_sig.params.push(AbiParam::new(types::I64));
        variant_eq_sig.params.push(AbiParam::new(types::I64));
        variant_eq_sig.returns.push(AbiParam::new(types::I64));
        let enum_variant_eq = module
            .declare_function("flux_jit_enum_variant_eq", Linkage::Import, &variant_eq_sig)
            .expect("declare flux_jit_enum_variant_eq");

        let mut payload_sig = module.make_signature();
        payload_sig.params.push(AbiParam::new(types::I64));
        payload_sig.returns.push(AbiParam::new(types::I64));
        payload_sig.returns.push(AbiParam::new(types::I64));
        let enum_payload_get = module
            .declare_function("flux_jit_enum_payload_get", Linkage::Import, &payload_sig)
            .expect("declare flux_jit_enum_payload_get");

        let mut builtin_sig = module.make_signature();
        builtin_sig.params.push(AbiParam::new(types::I64));
        builtin_sig.params.push(AbiParam::new(types::I64));
        builtin_sig.params.push(AbiParam::new(types::I64));
        builtin_sig.params.push(AbiParam::new(types::I64));
        builtin_sig.returns.push(AbiParam::new(types::I64));
        builtin_sig.returns.push(AbiParam::new(types::I64));
        let call_builtin = module
            .declare_function("flux_jit_call_builtin", Linkage::Import, &builtin_sig)
            .expect("declare flux_jit_call_builtin");

        let mut cast_sig = module.make_signature();
        for _ in 0..3 {
            cast_sig.params.push(AbiParam::new(types::I64));
        }
        cast_sig.returns.push(AbiParam::new(types::I64));
        cast_sig.returns.push(AbiParam::new(types::I64));
        let cast = module
            .declare_function("flux_jit_cast", Linkage::Import, &cast_sig)
            .expect("declare flux_jit_cast");

        Self {
            array_push,
            array_finish,
            array_len,
            array_get,
            range,
            emit,
            string_concat,
            value_eq,
            make_enum,
            enum_variant_eq,
            enum_payload_get,
            call_builtin,
            cast,
        }
    }
}

/// ABI of every compiled leaf: `fn(slots: *const u64) -> TaggedVal`
type ExprFn = unsafe extern "C" fn(*const u64) -> TaggedVal;

#[derive(Clone)]
pub struct CompiledExpr {
    pub func_id: FuncId,
    /// Raw function pointer, valid for the lifetime of `JitEngine`.
    pub ptr: *const u8,
}

unsafe impl Send for CompiledExpr {}
unsafe impl Sync for CompiledExpr {}

#[derive(Debug, Clone, Default)]
pub struct JitStats {
    pub compiled: usize,
    pub fallback: usize,
    pub reasons: HashMap<String, usize>,
    pub fallback_nodes: Vec<String>,
}

pub struct JitEngine {
    module: JITModule,
    pub layout: SlotLayout,
    pub exprs: HashMap<String, CompiledExpr>,
    pub stmts: HashMap<String, CompiledExpr>,
    pub vm_fallback: HashSet<String>,
    fallback_reasons: HashMap<String, usize>,
    pub lazy_if_deps: HashMap<String, HashSet<String>>,
    /// `__on_N` → slot pair index for event param binding.
    pub handler_param_slots: HashMap<String, u32>,
    pub event_indices: HashMap<String, u32>,
    pub tables: JitTables,
    host_slots: u32,
    /// Extra slot pairs beyond reactive nodes for JIT `Call` args / handler params.
    call_scratch_pairs: u32,
}

impl JitEngine {
    pub fn new(program: &CompiledProgram) -> Self {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("opt_level", "speed").unwrap();
        let isa = cranelift_native::builder()
            .unwrap_or_else(|e| panic!("unsupported host: {e}"))
            .finish(settings::Flags::new(flag_builder))
            .unwrap();
        let mut jit_builder = JITBuilder::with_isa(isa, default_libcall_names());
        jit_builder.symbol(
            "flux_jit_array_push",
            crate::host::flux_jit_array_push as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_array_finish",
            crate::host::flux_jit_array_finish as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_array_len",
            crate::host::flux_jit_array_len as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_array_get",
            crate::host::flux_jit_array_get as *const u8,
        );
        jit_builder.symbol("flux_jit_range", crate::host::flux_jit_range as *const u8);
        jit_builder.symbol("flux_jit_emit", crate::host::flux_jit_emit as *const u8);
        jit_builder.symbol(
            "flux_jit_string_concat",
            crate::host::flux_jit_string_concat as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_value_eq",
            crate::host::flux_jit_value_eq as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_make_enum",
            crate::host::flux_jit_make_enum as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_enum_variant_eq",
            crate::host::flux_jit_enum_variant_eq as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_enum_payload_get",
            crate::host::flux_jit_enum_payload_get as *const u8,
        );
        jit_builder.symbol(
            "flux_jit_call_builtin",
            crate::host::flux_jit_call_builtin as *const u8,
        );
        jit_builder.symbol("flux_jit_cast", crate::host::flux_jit_cast as *const u8);
        let module = JITModule::new(jit_builder);

        let tables = JitTables::build(program);

        let event_indices: HashMap<String, u32> = program
            .events
            .iter()
            .enumerate()
            .map(|(i, e)| (e.clone(), i as u32))
            .collect();

        let mut engine = Self {
            module,
            layout: SlotLayout::new(),
            exprs: HashMap::new(),
            stmts: HashMap::new(),
            vm_fallback: HashSet::new(),
            fallback_reasons: HashMap::new(),
            lazy_if_deps: HashMap::new(),
            handler_param_slots: HashMap::new(),
            event_indices,
            tables,
            host_slots: 0,
            call_scratch_pairs: 0,
        };

        let host_funcs = HostFuncs::declare(&mut engine.module);

        for node in &program.nodes {
            engine.layout.alloc(&node.name);
        }

        let host_slots = engine.layout.len() as u32;
        engine.host_slots = host_slots;
        let mut call_scratch_pairs = 0u32;
        let mut jit_fn_ids: HashMap<String, FuncId> = HashMap::new();
        for func in &program.functions {
            call_scratch_pairs = call_scratch_pairs
                .max(func.params.len() as u32)
                .max(max_call_argc_in_stmts(&func.body));
            if !func_is_numeric(&func.body, &HashSet::new()) {
                engine.note_fallback(&func.name, "non_numeric_fn_body");
                continue;
            }
            let label = format!("fn_{}", func.name);
            let mut ext_layout = engine.layout.name_to_idx.clone();
            for (i, p) in func.params.iter().enumerate() {
                ext_layout.insert(p.clone(), host_slots + i as u32);
            }
            match engine.compile_fn_stmts(
                &label,
                &func.body,
                &ext_layout,
                host_slots,
                &jit_fn_ids,
                &host_funcs,
            ) {
                Some((expr, fid)) => {
                    jit_fn_ids.insert(func.name.clone(), fid);
                    engine.exprs.insert(func.name.clone(), expr);
                }
                None => engine.note_fallback(&func.name, "compile_failed"),
            }
        }

        let jit_fn_names: HashSet<String> = jit_fn_ids.keys().cloned().collect();
        for node in &program.nodes {
            if node.code.is_empty() {
                continue;
            }
            call_scratch_pairs = call_scratch_pairs.max(max_call_argc_in_code(&node.code));
            if code_has_lazy_if(&node.code) {
                let deps = collect_lazy_if_loads(&node.code);
                if !deps.is_empty() {
                    engine.lazy_if_deps.insert(node.name.clone(), deps);
                }
            }
            let label = format!("expr_{}", node.name);
            if is_numeric_code(&node.code, &jit_fn_names) {
                match engine.compile_expr_code(
                    &label,
                    &node.code,
                    &engine.layout.name_to_idx.clone(),
                    host_slots,
                    &jit_fn_ids,
                    &host_funcs,
                ) {
                    Some((expr, _)) => {
                        engine.exprs.insert(node.name.clone(), expr);
                    }
                    None => engine.note_fallback(&node.name, "compile_failed"),
                }
            } else {
                engine.note_fallback(&node.name, "non_numeric_code");
            }
        }

        for (i, watch) in program.watches.iter().enumerate() {
            call_scratch_pairs = call_scratch_pairs.max(max_call_argc_in_stmts(&watch.body));
            if !stmts_numeric(&watch.body, &jit_fn_names) {
                continue;
            }
            let label = format!("stmt___watch_{i}");
            let key = format!("__watch_{i}");
            if let Some((expr, _)) = engine.compile_fn_stmts(
                &label,
                &watch.body,
                &engine.layout.name_to_idx.clone(),
                host_slots,
                &jit_fn_ids,
                &host_funcs,
            ) {
                engine.stmts.insert(key, expr);
            }
        }
        for (i, handler) in program.on_handlers.iter().enumerate() {
            call_scratch_pairs = call_scratch_pairs.max(max_call_argc_in_stmts(&handler.body));
            if !stmts_numeric(&handler.body, &jit_fn_names) {
                continue;
            }
            let label = format!("stmt___on_{i}");
            let key = format!("__on_{i}");
            let mut ext_layout = engine.layout.name_to_idx.clone();
            let compile_host_slots = if let Some(param) = &handler.param {
                ext_layout.insert(param.clone(), host_slots);
                engine.handler_param_slots.insert(key.clone(), host_slots);
                call_scratch_pairs = call_scratch_pairs.max(1);
                host_slots + 1
            } else {
                host_slots
            };
            if let Some((expr, _)) = engine.compile_fn_stmts(
                &label,
                &handler.body,
                &ext_layout,
                compile_host_slots,
                &jit_fn_ids,
                &host_funcs,
            ) {
                engine.stmts.insert(key, expr);
            }
        }

        engine.call_scratch_pairs = call_scratch_pairs;
        engine.module.finalize_definitions().unwrap();

        let ids: Vec<(String, FuncId)> = engine
            .exprs
            .iter()
            .chain(engine.stmts.iter())
            .map(|(n, e)| (n.clone(), e.func_id))
            .collect();
        for (name, fid) in ids {
            let ptr = engine.module.get_finalized_function(fid);
            if let Some(e) = engine.exprs.get_mut(&name) {
                e.ptr = ptr;
            } else if let Some(e) = engine.stmts.get_mut(&name) {
                e.ptr = ptr;
            }
        }

        engine
    }

    pub fn stats(&self) -> JitStats {
        JitStats {
            compiled: self.exprs.len() + self.stmts.len(),
            fallback: self.vm_fallback.len(),
            reasons: self.fallback_reasons.clone(),
            fallback_nodes: self.vm_fallback.iter().cloned().collect(),
        }
    }

    pub fn lazy_if_deps_for(&self, name: &str) -> Option<&HashSet<String>> {
        self.lazy_if_deps.get(name)
    }

    /// Reactive node slots (callee args start at this pair index).
    pub fn host_slot_count(&self) -> u32 {
        self.host_slots
    }

    /// Total slot pairs (`[tag, data]`) required for a JIT call buffer.
    pub fn slot_capacity_pairs(&self) -> u32 {
        self.host_slots + self.call_scratch_pairs
    }

    pub fn eval_expr_with(&self, name: &str, slots: &[u64]) -> Option<TaggedVal> {
        self.run_compiled(self.exprs.get(name)?, slots)
    }

    pub fn run_stmts_with(&self, key: &str, slots: &[u64]) -> Option<TaggedVal> {
        self.run_compiled(self.stmts.get(key)?, slots)
    }

    pub fn build_stmt_slots(&self, key: &str, base: &[u64], param: Option<TaggedVal>) -> Vec<u64> {
        let mut slots = base.to_vec();
        if let Some(tv) = param {
            if let Some(&pair_idx) = self.handler_param_slots.get(key) {
                let off = pair_idx as usize * 2;
                if slots.len() <= off + 1 {
                    slots.resize(off + 2, 0);
                }
                slots[off] = tv.tag;
                slots[off + 1] = tv.data;
            }
        }
        slots
    }

    pub fn call_fn_with_slots(&self, name: &str, slots: &[u64]) -> Option<TaggedVal> {
        self.run_compiled(self.exprs.get(name)?, slots)
    }

    pub fn call_fn(&self, name: &str, extra: &[TaggedVal]) -> Option<TaggedVal> {
        let e = self.exprs.get(name)?;
        let mut slots = self.layout.flat.clone();
        slots.resize(self.slot_capacity_pairs() as usize * 2, 0);
        for (i, tv) in extra.iter().enumerate() {
            let off = (self.host_slots as usize + i) * 2;
            slots[off] = tv.tag;
            slots[off + 1] = tv.data;
        }
        self.run_compiled(e, &slots)
    }

    fn run_compiled(&self, e: &CompiledExpr, slots: &[u64]) -> Option<TaggedVal> {
        let f: ExprFn = unsafe { std::mem::transmute(e.ptr) };
        Some(unsafe { f(slots.as_ptr()) })
    }

    fn note_fallback(&mut self, name: &str, reason: &str) {
        self.vm_fallback.insert(name.to_string());
        *self.fallback_reasons.entry(reason.to_string()).or_insert(0) += 1;
    }

    fn make_sig(&mut self) -> Signature {
        let ptr_ty = self.module.target_config().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_ty));
        sig.returns.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        sig
    }

    fn compile_expr_code(
        &mut self,
        label: &str,
        code: &[Inst],
        name_to_idx: &HashMap<String, u32>,
        host_slots: u32,
        jit_fns: &HashMap<String, FuncId>,
        host_funcs: &HostFuncs,
    ) -> Option<(CompiledExpr, FuncId)> {
        let sig = self.make_sig();
        let func_id = self
            .module
            .declare_function(label, Linkage::Local, &sig)
            .ok()?;
        let ptr_ty = self.module.target_config().pointer_type();

        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);

        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let slots_ptr = builder.block_params(entry)[0];
        let mut locals = HashMap::new();
        let (tag, data) = lower_insts(
            code,
            name_to_idx,
            slots_ptr,
            ptr_ty,
            host_slots,
            jit_fns,
            host_funcs,
            &self.event_indices,
            &self.tables,
            &mut locals,
            &mut self.module,
            &mut builder,
        )?;
        builder.ins().return_(&[tag, data]);
        builder.finalize();

        self.module.define_function(func_id, &mut ctx).ok()?;
        Some((
            CompiledExpr {
                func_id,
                ptr: std::ptr::null(),
            },
            func_id,
        ))
    }

    fn compile_fn_stmts(
        &mut self,
        label: &str,
        body: &[IrStmt],
        name_to_idx: &HashMap<String, u32>,
        host_slots: u32,
        jit_fns: &HashMap<String, FuncId>,
        host_funcs: &HostFuncs,
    ) -> Option<(CompiledExpr, FuncId)> {
        let sig = self.make_sig();
        let func_id = self
            .module
            .declare_function(label, Linkage::Local, &sig)
            .ok()?;
        let ptr_ty = self.module.target_config().pointer_type();

        let mut ctx = self.module.make_context();
        ctx.func.signature = sig;
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);

        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let slots_ptr = builder.block_params(entry)[0];
        let mut locals = HashMap::new();
        let (tag, data) = lower_stmts(
            body,
            name_to_idx,
            slots_ptr,
            ptr_ty,
            host_slots,
            jit_fns,
            host_funcs,
            &self.event_indices,
            &self.tables,
            &mut locals,
            &mut self.module,
            &mut builder,
        )?;
        builder.ins().return_(&[tag, data]);
        builder.finalize();

        self.module.define_function(func_id, &mut ctx).ok()?;
        Some((
            CompiledExpr {
                func_id,
                ptr: std::ptr::null(),
            },
            func_id,
        ))
    }
}

fn code_has_lazy_if(code: &[Inst]) -> bool {
    code.iter().any(|i| match i {
        Inst::LazyIf { .. } => true,
        Inst::ShortAnd(inner) | Inst::ShortOr(inner) => code_has_lazy_if(inner),
        _ => false,
    })
}

fn collect_lazy_if_loads(code: &[Inst]) -> HashSet<String> {
    let mut set = HashSet::new();
    collect_lazy_if_loads_rec(code, &mut set);
    set
}

fn collect_lazy_if_loads_rec(code: &[Inst], set: &mut HashSet<String>) {
    for inst in code {
        match inst {
            Inst::Load(name) => {
                set.insert(name.clone());
            }
            Inst::LazyIf {
                then_branch,
                else_branch,
            } => {
                collect_lazy_if_loads_rec(then_branch, set);
                collect_lazy_if_loads_rec(else_branch, set);
            }
            Inst::ShortAnd(inner) | Inst::ShortOr(inner) => collect_lazy_if_loads_rec(inner, set),
            _ => {}
        }
    }
}
