use std::collections::HashMap;

use cranelift::codegen::ir::BlockArg;
use cranelift::prelude::*;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

use flux_ir::Inst;

use super::emit::{
    emit_add, emit_cmp_ordered, emit_const, emit_div, emit_icmp_eq, emit_icmp_ne, emit_mul,
    emit_neg, emit_sub, slot_data_addr, slot_tag_addr, store_tagged,
};
use super::engine::HostFuncs;
use super::tables::JitTables;

/// Lower a flat stack-machine `Vec<Inst>` → `(tag, data)` values in Cranelift.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::only_used_in_recursion)]
pub(crate) fn lower_insts(
    code: &[Inst],
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
    let mut stack: Vec<(Value, Value)> = Vec::new();

    macro_rules! pop2 {
        () => {{
            let r = stack.pop()?;
            let l = stack.pop()?;
            (l, r)
        }};
    }

    for inst in code {
        match inst {
            Inst::Const(v) => stack.push(emit_const(v, tables, b)?),

            Inst::Load(name) => {
                if let Some(pair) = locals.get(name) {
                    stack.push(*pair);
                } else {
                    let idx = *layout.get(name.as_str())?;
                    let tag_addr = slot_tag_addr(slots_ptr, idx, ptr_ty, b);
                    let data_addr = slot_data_addr(slots_ptr, idx, ptr_ty, b);
                    let tag = b
                        .ins()
                        .load(types::I64, MemFlagsData::trusted(), tag_addr, 0);
                    let data = b
                        .ins()
                        .load(types::I64, MemFlagsData::trusted(), data_addr, 0);
                    stack.push((tag, data));
                }
            }

            Inst::Add => {
                let (lhs, rhs) = pop2!();
                stack.push(emit_add(lhs, rhs, host_funcs.string_concat, module, b));
            }
            Inst::Sub => {
                let (lhs, rhs) = pop2!();
                stack.push(emit_sub(lhs, rhs, b));
            }
            Inst::Mul => {
                let (lhs, rhs) = pop2!();
                stack.push(emit_mul(lhs, rhs, b));
            }
            Inst::Div => {
                let (lhs, rhs) = pop2!();
                stack.push(emit_div(lhs, rhs, b));
            }
            Inst::Neg => {
                let pair = stack.pop()?;
                stack.push(emit_neg(pair, b));
            }
            Inst::Not => {
                let (_, ai) = stack.pop()?;
                let zero = b.ins().iconst(types::I64, 0);
                let cmp = b.ins().icmp(IntCC::Equal, ai, zero);
                let res = b.ins().uextend(types::I64, cmp);
                let tag3 = b.ins().iconst(types::I64, 3);
                stack.push((tag3, res));
            }

            Inst::CmpEq => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(emit_icmp_eq(lhs, rhs, host_funcs.value_eq, module, b)?);
            }
            Inst::CmpNe => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(emit_icmp_ne(lhs, rhs, host_funcs.value_eq, module, b)?);
            }
            Inst::CmpLt => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(emit_cmp_ordered(
                    IntCC::SignedLessThan,
                    FloatCC::LessThan,
                    lhs,
                    rhs,
                    b,
                )?);
            }
            Inst::CmpLe => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(emit_cmp_ordered(
                    IntCC::SignedLessThanOrEqual,
                    FloatCC::LessThanOrEqual,
                    lhs,
                    rhs,
                    b,
                )?);
            }
            Inst::CmpGt => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(emit_cmp_ordered(
                    IntCC::SignedGreaterThan,
                    FloatCC::GreaterThan,
                    lhs,
                    rhs,
                    b,
                )?);
            }
            Inst::CmpGe => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                stack.push(emit_cmp_ordered(
                    IntCC::SignedGreaterThanOrEqual,
                    FloatCC::GreaterThanOrEqual,
                    lhs,
                    rhs,
                    b,
                )?);
            }

            Inst::ShortAnd(right_code) => {
                let (_, ai) = stack.pop()?;
                let zero = b.ins().iconst(types::I64, 0);
                let is_true = b.ins().icmp(IntCC::NotEqual, ai, zero);

                let then_bb = b.create_block();
                let merge_bb = b.create_block();
                b.append_block_param(merge_bb, types::I64);
                b.append_block_param(merge_bb, types::I64);

                let false_tag = b.ins().iconst(types::I64, 3);
                let false_data = b.ins().iconst(types::I64, 0);

                b.ins().brif(
                    is_true,
                    then_bb,
                    &[] as &[BlockArg],
                    merge_bb,
                    &[BlockArg::Value(false_tag), BlockArg::Value(false_data)],
                );
                b.switch_to_block(then_bb);
                b.seal_block(then_bb);
                let (rt, rd) = lower_insts(
                    right_code,
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
                b.ins()
                    .jump(merge_bb, &[BlockArg::Value(rt), BlockArg::Value(rd)]);
                b.switch_to_block(merge_bb);
                b.seal_block(merge_bb);
                let tag = b.block_params(merge_bb)[0];
                let data = b.block_params(merge_bb)[1];
                stack.push((tag, data));
            }

            Inst::ShortOr(right_code) => {
                let (_, ai) = stack.pop()?;
                let zero = b.ins().iconst(types::I64, 0);
                let is_true = b.ins().icmp(IntCC::NotEqual, ai, zero);

                let else_bb = b.create_block();
                let merge_bb = b.create_block();
                b.append_block_param(merge_bb, types::I64);
                b.append_block_param(merge_bb, types::I64);

                let true_tag = b.ins().iconst(types::I64, 3);
                let true_data = b.ins().iconst(types::I64, 1);

                b.ins().brif(
                    is_true,
                    merge_bb,
                    &[BlockArg::Value(true_tag), BlockArg::Value(true_data)],
                    else_bb,
                    &[] as &[BlockArg],
                );
                b.switch_to_block(else_bb);
                b.seal_block(else_bb);
                let (rt, rd) = lower_insts(
                    right_code,
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
                b.ins()
                    .jump(merge_bb, &[BlockArg::Value(rt), BlockArg::Value(rd)]);
                b.switch_to_block(merge_bb);
                b.seal_block(merge_bb);
                let tag = b.block_params(merge_bb)[0];
                let data = b.block_params(merge_bb)[1];
                stack.push((tag, data));
            }

            Inst::LazyIf {
                then_branch,
                else_branch,
            } => {
                let (_, cond_d) = stack.pop()?;
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
                let (then_t, then_d) = lower_insts(
                    then_branch,
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
                let (else_t, else_d) = lower_insts(
                    else_branch,
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
                    &[BlockArg::Value(else_t), BlockArg::Value(else_d)],
                );
                b.switch_to_block(merge_bb);
                b.seal_block(merge_bb);
                let tag = b.block_params(merge_bb)[0];
                let data = b.block_params(merge_bb)[1];
                stack.push((tag, data));
            }

            Inst::Select => {
                let (else_t, else_d) = stack.pop()?;
                let (then_t, then_d) = stack.pop()?;
                let (_, cond_d) = stack.pop()?;
                let zero = b.ins().iconst(types::I64, 0);
                let is_true = b.ins().icmp(IntCC::NotEqual, cond_d, zero);
                let tag = b.ins().select(is_true, then_t, else_t);
                let data = b.ins().select(is_true, then_d, else_d);
                stack.push((tag, data));
            }

            Inst::Call { name, argc } if name == "range" => {
                let mut args = Vec::new();
                for _ in 0..*argc {
                    args.push(stack.pop()?);
                }
                args.reverse();
                let zero = b.ins().iconst(types::I64, 0);
                let a = args.first().map(|(_, d)| *d).unwrap_or(zero);
                let bb = args.get(1).map(|(_, d)| *d).unwrap_or(zero);
                let c = args.get(2).map(|(_, d)| *d).unwrap_or(zero);
                let argc_v = b.ins().iconst(types::I64, *argc as i64);
                let range_ref = module.declare_func_in_func(host_funcs.range, b.func);
                let call = b.ins().call(range_ref, &[a, bb, c, argc_v]);
                let results = b.inst_results(call);
                stack.push((results[0], results[1]));
            }

            Inst::Call { name, argc }
                if flux_std::PURE_INTRINSICS.contains(&name.as_str())
                    && !jit_fns.contains_key(name) =>
            {
                let mut args = Vec::new();
                for _ in 0..*argc {
                    args.push(stack.pop()?);
                }
                args.reverse();
                for (i, (tag, data)) in args.iter().enumerate() {
                    store_tagged(slots_ptr, host_slots + i as u32, *tag, *data, ptr_ty, b);
                }
                let name_id = tables.string_id(name)?;
                let name_v = b.ins().iconst(types::I64, name_id as i64);
                let base_v = b.ins().iconst(types::I64, host_slots as i64);
                let argc_v = b.ins().iconst(types::I64, *argc as i64);
                let builtin_ref = module.declare_func_in_func(host_funcs.call_builtin, b.func);
                let call = b
                    .ins()
                    .call(builtin_ref, &[name_v, slots_ptr, base_v, argc_v]);
                let results = b.inst_results(call);
                stack.push((results[0], results[1]));
            }

            Inst::Cast { ty } => {
                let (tag, data) = stack.pop()?;
                let ty_id = tables.string_id(ty)?;
                let ty_v = b.ins().iconst(types::I64, ty_id as i64);
                let cast_ref = module.declare_func_in_func(host_funcs.cast, b.func);
                let call = b.ins().call(cast_ref, &[tag, data, ty_v]);
                let results = b.inst_results(call);
                stack.push((results[0], results[1]));
            }

            Inst::Call { name, argc } => {
                let callee = jit_fns.get(name)?;
                let mut args = Vec::new();
                for _ in 0..*argc {
                    args.push(stack.pop()?);
                }
                args.reverse();
                for (i, (tag, data)) in args.iter().enumerate() {
                    store_tagged(slots_ptr, host_slots + i as u32, *tag, *data, ptr_ty, b);
                }
                let func_ref = module.declare_func_in_func(*callee, b.func);
                let call = b.ins().call(func_ref, &[slots_ptr]);
                let results = b.inst_results(call);
                stack.push((results[0], results[1]));
            }

            Inst::MakeArray(count) => {
                let mut pairs = Vec::new();
                for _ in 0..*count {
                    pairs.push(stack.pop()?);
                }
                pairs.reverse();
                for (tag, data) in pairs {
                    let push_ref = module.declare_func_in_func(host_funcs.array_push, b.func);
                    b.ins().call(push_ref, &[tag, data]);
                }
                let count_v = b.ins().iconst(types::I64, *count as i64);
                let finish_ref = module.declare_func_in_func(host_funcs.array_finish, b.func);
                let call = b.ins().call(finish_ref, &[count_v]);
                let results = b.inst_results(call);
                stack.push((results[0], results[1]));
            }

            Inst::EnumTagEq(variant) | Inst::VariantTagEq(variant) => {
                let (_, enum_d) = stack.pop()?;
                let name_id = tables.string_id(variant)?;
                let variant_v = b.ins().iconst(types::I64, name_id as i64);
                let eq_ref = module.declare_func_in_func(host_funcs.enum_variant_eq, b.func);
                let call = b.ins().call(eq_ref, &[enum_d, variant_v]);
                let data = b.inst_results(call)[0];
                let tag = b.ins().iconst(types::I64, 3);
                stack.push((tag, data));
            }
            Inst::VariantPayloadGet { field: _ } => {
                let (_, enum_d) = stack.pop()?;
                let payload_ref = module.declare_func_in_func(host_funcs.enum_payload_get, b.func);
                let call = b.ins().call(payload_ref, &[enum_d]);
                let results = b.inst_results(call);
                stack.push((results[0], results[1]));
            }
            Inst::BindLocal(name) => {
                let val = stack.pop()?;
                locals.insert(name.clone(), val);
            }
            Inst::MakeUserEnum {
                type_name,
                variant,
                has_payload,
            } => {
                let (payload_tag, payload_data) = if *has_payload {
                    stack.pop()?
                } else {
                    let zero = b.ins().iconst(types::I64, 0);
                    (zero, zero)
                };
                let type_id = b
                    .ins()
                    .iconst(types::I64, tables.string_id(type_name)? as i64);
                let variant_id = b
                    .ins()
                    .iconst(types::I64, tables.string_id(variant)? as i64);
                let has_p = b.ins().iconst(types::I64, *has_payload as i64);
                let make_ref = module.declare_func_in_func(host_funcs.make_enum, b.func);
                let call = b.ins().call(
                    make_ref,
                    &[type_id, variant_id, has_p, payload_tag, payload_data],
                );
                let results = b.inst_results(call);
                stack.push((results[0], results[1]));
            }

            Inst::MakeUserStruct { .. } | Inst::FieldGet(_) => return None,
        }
    }

    stack.pop()
}
