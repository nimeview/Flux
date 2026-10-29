use std::collections::HashMap;

use cranelift::codegen::ir::BlockArg;
use cranelift::prelude::*;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use flux_ir::IrStmt;

use super::emit::emit_bool_test;
use super::engine::HostFuncs;
use super::lower_inst::lower_insts;
use super::tables::JitTables;

/// Lower a sequence of `IrStmt` and return the last expression value.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_stmts(
    stmts: &[IrStmt],
    layout: &HashMap<String, u32>,
    slots_ptr: Value,
    ptr_ty: Type,
    host_slots: u32,
    jit_fns: &HashMap<String, FuncId>,
    host_funcs: &HostFuncs,
    event_indices: &HashMap<String, u32>,
    tables: &JitTables,
    locals: &mut HashMap<String, (Value, Value)>,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> Option<(Value, Value)> {
    let unit_tag = b.ins().iconst(types::I64, 0);
    let unit_data = b.ins().iconst(types::I64, 0);
    let mut last = (unit_tag, unit_data);

    for stmt in stmts {
        match stmt {
            IrStmt::Expr { code } => {
                last = lower_insts(
                    code,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
            }
            IrStmt::Let { name, expr } => {
                last = lower_insts(
                    expr,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                locals.insert(name.clone(), last);
            }
            IrStmt::Assign { target, expr } => {
                let (tag, data) = lower_insts(
                    expr,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                if let Some(&idx) = layout.get(target.as_str()) {
                    super::emit::store_tagged(slots_ptr, idx, tag, data, ptr_ty, b);
                }
                if locals.contains_key(target) {
                    locals.insert(target.clone(), (tag, data));
                }
                last = (tag, data);
            }
            IrStmt::Emit { event, arg } => {
                let (tag, data) = if let Some(code) = arg {
                    lower_insts(
                        code,
                        layout,
                        slots_ptr,
                        ptr_ty,
                        host_slots,
                        jit_fns,
                        host_funcs,
                        event_indices,
                        tables,
                        locals,
                        module,
                        b,
                    )?
                } else {
                    (unit_tag, unit_data)
                };
                let event_idx = *event_indices.get(event)?;
                let idx_v = b.ins().iconst(types::I64, event_idx as i64);
                let emit_ref = module.declare_func_in_func(host_funcs.emit, b.func);
                b.ins().call(emit_ref, &[idx_v, tag, data]);
                last = (unit_tag, unit_data);
            }
            IrStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                let (_, cond_d) = lower_insts(
                    cond,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                let zero = b.ins().iconst(types::I64, 0);
                let is_true = b.ins().icmp(IntCC::NotEqual, cond_d, zero);

                let then_bb = b.create_block();
                let else_bb = b.create_block();
                let merge_bb = b.create_block();
                b.append_block_param(merge_bb, types::I64);
                b.append_block_param(merge_bb, types::I64);

                b.ins().brif(
                    is_true,
                    then_bb,
                    &[] as &[BlockArg],
                    else_bb,
                    &[] as &[BlockArg],
                );
                b.switch_to_block(then_bb);
                b.seal_block(then_bb);
                let (then_t, then_d) = lower_stmts(
                    then_body,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                b.ins().jump(
                    merge_bb,
                    &[BlockArg::Value(then_t), BlockArg::Value(then_d)],
                );
                b.switch_to_block(else_bb);
                b.seal_block(else_bb);
                let (else_t, else_d) = if let Some(body) = else_body {
                    lower_stmts(
                        body,
                        layout,
                        slots_ptr,
                        ptr_ty,
                        host_slots,
                        jit_fns,
                        host_funcs,
                        event_indices,
                        tables,
                        locals,
                        module,
                        b,
                    )?
                } else {
                    (unit_tag, unit_data)
                };
                b.ins().jump(
                    merge_bb,
                    &[BlockArg::Value(else_t), BlockArg::Value(else_d)],
                );
                b.switch_to_block(merge_bb);
                b.seal_block(merge_bb);
                last = (b.block_params(merge_bb)[0], b.block_params(merge_bb)[1]);
            }
            IrStmt::While { cond, body } => {
                let header_bb = b.create_block();
                let body_bb = b.create_block();
                let exit_bb = b.create_block();
                b.ins().jump(header_bb, &[] as &[BlockArg]);
                b.switch_to_block(header_bb);
                let (cond_t, cond_d) = lower_insts(
                    cond,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                let is_true = emit_bool_test(cond_t, cond_d, b);
                b.ins().brif(
                    is_true,
                    body_bb,
                    &[] as &[BlockArg],
                    exit_bb,
                    &[] as &[BlockArg],
                );
                b.switch_to_block(body_bb);
                b.seal_block(body_bb);
                last = lower_stmts(
                    body,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                b.ins().jump(header_bb, &[] as &[BlockArg]);
                b.switch_to_block(exit_bb);
                b.seal_block(header_bb);
                b.seal_block(exit_bb);
            }
            IrStmt::For { var, iter, body } => {
                let (arr_tag, arr_data) = lower_insts(
                    iter,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                let len_ref = module.declare_func_in_func(host_funcs.array_len, b.func);
                let len_call = b.ins().call(len_ref, &[arr_data]);
                let len = b.inst_results(len_call)[0];

                let header_bb = b.create_block();
                b.append_block_param(header_bb, types::I64);
                let body_bb = b.create_block();
                let exit_bb = b.create_block();
                let zero = b.ins().iconst(types::I64, 0);
                b.ins().jump(header_bb, &[BlockArg::Value(zero)]);
                b.switch_to_block(header_bb);
                let idx = b.block_params(header_bb)[0];
                let cond = b.ins().icmp(IntCC::SignedLessThan, idx, len);
                b.ins().brif(
                    cond,
                    body_bb,
                    &[] as &[BlockArg],
                    exit_bb,
                    &[] as &[BlockArg],
                );
                b.switch_to_block(body_bb);
                b.seal_block(body_bb);
                let get_ref = module.declare_func_in_func(host_funcs.array_get, b.func);
                let get_call = b.ins().call(get_ref, &[arr_data, idx]);
                let results = b.inst_results(get_call);
                locals.insert(var.clone(), (results[0], results[1]));
                let _ = arr_tag;
                last = lower_stmts(
                    body,
                    layout,
                    slots_ptr,
                    ptr_ty,
                    host_slots,
                    jit_fns,
                    host_funcs,
                    event_indices,
                    tables,
                    locals,
                    module,
                    b,
                )?;
                locals.remove(var);
                let one = b.ins().iconst(types::I64, 1);
                let next = b.ins().iadd(idx, one);
                b.ins().jump(header_bb, &[BlockArg::Value(next)]);
                b.switch_to_block(exit_bb);
                b.seal_block(header_bb);
                b.seal_block(exit_bb);
            }
            IrStmt::Return { expr } => {
                last = if let Some(code) = expr {
                    lower_insts(
                        code,
                        layout,
                        slots_ptr,
                        ptr_ty,
                        host_slots,
                        jit_fns,
                        host_funcs,
                        event_indices,
                        tables,
                        locals,
                        module,
                        b,
                    )?
                } else {
                    (unit_tag, unit_data)
                };
            }
            IrStmt::Break | IrStmt::Continue => return None,
        }
    }

    Some(last)
}
