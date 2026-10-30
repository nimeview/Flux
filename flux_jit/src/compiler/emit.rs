use cranelift::codegen::ir::{BlockArg, Endianness};
use cranelift::prelude::*;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};
use flux_ir::IrValue;

use super::tables::JitTables;
use flux_value::{int_tag, IntWidth, NumOp, TAG_BOOL, TAG_ERROR, TAG_FLOAT, TAG_INT, TAG_STRING};

fn float_bits_flags() -> MemFlagsData {
    MemFlagsData::new().with_endianness(Endianness::Little)
}

pub(super) fn slot_pair_addr(
    base: Value,
    pair_idx: u32,
    sub: u32,
    ptr_ty: Type,
    b: &mut FunctionBuilder<'_>,
) -> Value {
    let byte_off = (pair_idx as i64 * 2 + sub as i64) * 8;
    if byte_off == 0 {
        base
    } else {
        let off = b.ins().iconst(ptr_ty, byte_off);
        b.ins().iadd(base, off)
    }
}

pub(super) fn slot_tag_addr(
    base: Value,
    pair_idx: u32,
    ptr_ty: Type,
    b: &mut FunctionBuilder<'_>,
) -> Value {
    slot_pair_addr(base, pair_idx, 0, ptr_ty, b)
}

pub(super) fn slot_data_addr(
    base: Value,
    pair_idx: u32,
    ptr_ty: Type,
    b: &mut FunctionBuilder<'_>,
) -> Value {
    slot_pair_addr(base, pair_idx, 1, ptr_ty, b)
}

pub(super) fn store_tagged(
    slots_ptr: Value,
    pair_idx: u32,
    tag: Value,
    data: Value,
    ptr_ty: Type,
    b: &mut FunctionBuilder<'_>,
) {
    let tag_addr = slot_tag_addr(slots_ptr, pair_idx, ptr_ty, b);
    let data_addr = slot_data_addr(slots_ptr, pair_idx, ptr_ty, b);
    b.ins().store(MemFlagsData::trusted(), tag, tag_addr, 0);
    b.ins().store(MemFlagsData::trusted(), data, data_addr, 0);
}

pub(super) fn emit_const(
    v: &IrValue,
    tables: &JitTables,
    b: &mut FunctionBuilder<'_>,
) -> Option<(Value, Value)> {
    match v {
        IrValue::Int(n) => {
            let (tag, data) = if let Ok(signed) = i64::try_from(*n) {
                (TAG_INT as i64, signed)
            } else if let Ok(unsigned) = u64::try_from(*n) {
                (int_tag(IntWidth::U64) as i64, unsigned as i64)
            } else {
                return None;
            };
            let tag = b.ins().iconst(types::I64, tag);
            let data = b.ins().iconst(types::I64, data);
            Some((tag, data))
        }
        IrValue::Float(f) => {
            let fv = b.ins().f64const(*f);
            let bits = b.ins().bitcast(types::I64, float_bits_flags(), fv);
            let tag = b.ins().iconst(types::I64, 2);
            Some((tag, bits))
        }
        IrValue::Bool(bv) => {
            let tag = b.ins().iconst(types::I64, 3);
            let data = b.ins().iconst(types::I64, *bv as i64);
            Some((tag, data))
        }
        IrValue::String(s) => {
            let id = tables.string_id(s)?;
            let tag = b.ins().iconst(types::I64, TAG_STRING as i64);
            let data = b.ins().iconst(types::I64, id as i64);
            Some((tag, data))
        }
        IrValue::Unit => {
            let tag = b.ins().iconst(types::I64, 0);
            let data = b.ins().iconst(types::I64, 0);
            Some((tag, data))
        }
    }
}

pub(super) fn emit_bool_test(tag: Value, data: Value, b: &mut FunctionBuilder<'_>) -> Value {
    let tag_bool = b.ins().iconst(types::I64, TAG_BOOL as i64);
    let tag_ok = b.ins().icmp(IntCC::Equal, tag, tag_bool);
    let zero = b.ins().iconst(types::I64, 0);
    let data_ok = b.ins().icmp(IntCC::NotEqual, data, zero);
    b.ins().band(tag_ok, data_ok)
}

fn tag_is_int(tag: Value, b: &mut FunctionBuilder<'_>) -> Value {
    let one = b.ins().iconst(types::I64, 1);
    b.ins().icmp(IntCC::Equal, tag, one)
}

fn tag_is(tag: Value, want: u64, b: &mut FunctionBuilder<'_>) -> Value {
    b.ins().icmp_imm(IntCC::Equal, tag, want as i64)
}

fn both_tag(lt: Value, rt: Value, want: u64, b: &mut FunctionBuilder<'_>) -> Value {
    let l = tag_is(lt, want, b);
    let r = tag_is(rt, want, b);
    b.ins().band(l, r)
}

fn call_pair(
    func: FuncId,
    args: &[Value],
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> (Value, Value) {
    let func_ref = module.declare_func_in_func(func, b.func);
    let call = b.ins().call(func_ref, args);
    let results = b.inst_results(call);
    (results[0], results[1])
}

fn merge_pair(b: &mut FunctionBuilder<'_>) -> Block {
    let merge_bb = b.create_block();
    b.append_block_param(merge_bb, types::I64);
    b.append_block_param(merge_bb, types::I64);
    merge_bb
}

fn jump_pair(merge_bb: Block, tag: Value, data: Value, b: &mut FunctionBuilder<'_>) {
    b.ins()
        .jump(merge_bb, &[BlockArg::Value(tag), BlockArg::Value(data)]);
}

/// `+ - * / %` on numbers. Inline fast paths cover `Int64 op Int64` (with
/// overflow and zero-divisor checks) and `Float64 op Float64`; every other mix,
/// and every fast-path failure, calls `flux_jit_arith`, which shares the VM's
/// semantics and returns a `TAG_ERROR` value on failure.
pub(super) fn emit_arith(
    op: NumOp,
    (lt, ld): (Value, Value),
    (rt, rd): (Value, Value),
    arith: FuncId,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> (Value, Value) {
    let int_bb = b.create_block();
    let check_float_bb = b.create_block();
    let float_bb = b.create_block();
    let slow_bb = b.create_block();
    let merge_bb = merge_pair(b);

    let is_int = both_tag(lt, rt, TAG_INT, b);
    b.ins().brif(
        is_int,
        int_bb,
        &[] as &[BlockArg],
        check_float_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(int_bb);
    b.seal_block(int_bb);
    let itag = b.ins().iconst(types::I64, TAG_INT as i64);
    match op {
        NumOp::Add | NumOp::Sub | NumOp::Mul => {
            let (r, overflow) = match op {
                NumOp::Add => b.ins().sadd_overflow(ld, rd),
                NumOp::Sub => b.ins().ssub_overflow(ld, rd),
                _ => b.ins().smul_overflow(ld, rd),
            };
            b.ins().brif(
                overflow,
                slow_bb,
                &[] as &[BlockArg],
                merge_bb,
                &[BlockArg::Value(itag), BlockArg::Value(r)],
            );
        }
        NumOp::Div | NumOp::Rem => {
            // `sdiv`/`srem` trap on a zero divisor and on MIN / -1: route both
            // to the slow path, which reports the language error instead.
            let zero = b.ins().icmp_imm(IntCC::Equal, rd, 0);
            let lhs_min = b.ins().icmp_imm(IntCC::Equal, ld, i64::MIN);
            let rhs_neg1 = b.ins().icmp_imm(IntCC::Equal, rd, -1);
            let min_neg1 = b.ins().band(lhs_min, rhs_neg1);
            let unsafe_div = b.ins().bor(zero, min_neg1);
            let div_bb = b.create_block();
            b.ins().brif(
                unsafe_div,
                slow_bb,
                &[] as &[BlockArg],
                div_bb,
                &[] as &[BlockArg],
            );
            b.switch_to_block(div_bb);
            b.seal_block(div_bb);
            let r = if op == NumOp::Div {
                b.ins().sdiv(ld, rd)
            } else {
                b.ins().srem(ld, rd)
            };
            jump_pair(merge_bb, itag, r, b);
        }
    }

    b.switch_to_block(check_float_bb);
    b.seal_block(check_float_bb);
    let is_float = both_tag(lt, rt, TAG_FLOAT, b);
    // Cranelift has no float remainder instruction; `%` on floats is slow-path only.
    let float_fast = if op == NumOp::Rem {
        b.ins().iconst(types::I8, 0)
    } else {
        is_float
    };
    b.ins().brif(
        float_fast,
        float_bb,
        &[] as &[BlockArg],
        slow_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(float_bb);
    b.seal_block(float_bb);
    let af = b.ins().bitcast(types::F64, float_bits_flags(), ld);
    let bf = b.ins().bitcast(types::F64, float_bits_flags(), rd);
    let rf = match op {
        NumOp::Add => b.ins().fadd(af, bf),
        NumOp::Sub => b.ins().fsub(af, bf),
        NumOp::Mul => b.ins().fmul(af, bf),
        _ => b.ins().fdiv(af, bf),
    };
    let bits = b.ins().bitcast(types::I64, float_bits_flags(), rf);
    let ftag = b.ins().iconst(types::I64, TAG_FLOAT as i64);
    jump_pair(merge_bb, ftag, bits, b);

    b.switch_to_block(slow_bb);
    b.seal_block(slow_bb);
    let op_v = b.ins().iconst(types::I64, op as i64);
    let (st, sd) = call_pair(arith, &[op_v, lt, ld, rt, rd], module, b);
    jump_pair(merge_bb, st, sd, b);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    (b.block_params(merge_bb)[0], b.block_params(merge_bb)[1])
}

/// `+`: string concatenation when both operands are strings, else [`emit_arith`].
pub(super) fn emit_add(
    lhs: (Value, Value),
    rhs: (Value, Value),
    string_concat: FuncId,
    arith: FuncId,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> (Value, Value) {
    let str_bb = b.create_block();
    let num_bb = b.create_block();
    let merge_bb = merge_pair(b);

    let is_str = both_tag(lhs.0, rhs.0, TAG_STRING, b);
    b.ins().brif(
        is_str,
        str_bb,
        &[] as &[BlockArg],
        num_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(str_bb);
    b.seal_block(str_bb);
    let (st, sd) = call_pair(string_concat, &[lhs.0, lhs.1, rhs.0, rhs.1], module, b);
    jump_pair(merge_bb, st, sd, b);

    b.switch_to_block(num_bb);
    b.seal_block(num_bb);
    let (nt, nd) = emit_arith(NumOp::Add, lhs, rhs, arith, module, b);
    jump_pair(merge_bb, nt, nd, b);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    (b.block_params(merge_bb)[0], b.block_params(merge_bb)[1])
}

pub(super) fn emit_neg(
    (tag, data): (Value, Value),
    neg: FuncId,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> (Value, Value) {
    let int_bb = b.create_block();
    let check_float_bb = b.create_block();
    let float_bb = b.create_block();
    let slow_bb = b.create_block();
    let merge_bb = merge_pair(b);

    // `-i64::MIN` overflows: send it to the slow path with the other mixes.
    let is_int = tag_is(tag, TAG_INT, b);
    let not_min = b.ins().icmp_imm(IntCC::NotEqual, data, i64::MIN);
    let int_fast = b.ins().band(is_int, not_min);
    b.ins().brif(
        int_fast,
        int_bb,
        &[] as &[BlockArg],
        check_float_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(int_bb);
    b.seal_block(int_bb);
    let r = b.ins().ineg(data);
    let itag = b.ins().iconst(types::I64, TAG_INT as i64);
    jump_pair(merge_bb, itag, r, b);

    b.switch_to_block(check_float_bb);
    b.seal_block(check_float_bb);
    let is_float = tag_is(tag, TAG_FLOAT, b);
    b.ins().brif(
        is_float,
        float_bb,
        &[] as &[BlockArg],
        slow_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(float_bb);
    b.seal_block(float_bb);
    let af = b.ins().bitcast(types::F64, float_bits_flags(), data);
    let rf = b.ins().fneg(af);
    let bits = b.ins().bitcast(types::I64, float_bits_flags(), rf);
    let ftag = b.ins().iconst(types::I64, TAG_FLOAT as i64);
    jump_pair(merge_bb, ftag, bits, b);

    b.switch_to_block(slow_bb);
    b.seal_block(slow_bb);
    let (st, sd) = call_pair(neg, &[tag, data], module, b);
    jump_pair(merge_bb, st, sd, b);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    (b.block_params(merge_bb)[0], b.block_params(merge_bb)[1])
}

/// Ordered comparison: inline for `Int64`/`Int64` and `Float64`/`Float64`,
/// `flux_jit_cmp` for mixed widths or kinds. `host_op`: 0 `<`, 1 `<=`, 2 `>`, 3 `>=`.
pub(super) fn emit_cmp_ordered(
    host_op: i64,
    (lt, ld): (Value, Value),
    (rt, rd): (Value, Value),
    cmp: FuncId,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> Option<(Value, Value)> {
    let (cc_int, cc_float) = match host_op {
        0 => (IntCC::SignedLessThan, FloatCC::LessThan),
        1 => (IntCC::SignedLessThanOrEqual, FloatCC::LessThanOrEqual),
        2 => (IntCC::SignedGreaterThan, FloatCC::GreaterThan),
        _ => (IntCC::SignedGreaterThanOrEqual, FloatCC::GreaterThanOrEqual),
    };
    let int_bb = b.create_block();
    let check_float_bb = b.create_block();
    let float_bb = b.create_block();
    let slow_bb = b.create_block();
    let merge_bb = b.create_block();
    b.append_block_param(merge_bb, types::I64);

    let is_int = both_tag(lt, rt, TAG_INT, b);
    b.ins().brif(
        is_int,
        int_bb,
        &[] as &[BlockArg],
        check_float_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(int_bb);
    b.seal_block(int_bb);
    let c = b.ins().icmp(cc_int, ld, rd);
    let data = b.ins().uextend(types::I64, c);
    b.ins().jump(merge_bb, &[BlockArg::Value(data)]);

    b.switch_to_block(check_float_bb);
    b.seal_block(check_float_bb);
    let is_float = both_tag(lt, rt, TAG_FLOAT, b);
    b.ins().brif(
        is_float,
        float_bb,
        &[] as &[BlockArg],
        slow_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(float_bb);
    b.seal_block(float_bb);
    let af = b.ins().bitcast(types::F64, float_bits_flags(), ld);
    let bf = b.ins().bitcast(types::F64, float_bits_flags(), rd);
    let c = b.ins().fcmp(cc_float, af, bf);
    let data = b.ins().uextend(types::I64, c);
    b.ins().jump(merge_bb, &[BlockArg::Value(data)]);

    b.switch_to_block(slow_bb);
    b.seal_block(slow_bb);
    let op_v = b.ins().iconst(types::I64, host_op);
    let cmp_ref = module.declare_func_in_func(cmp, b.func);
    let call = b.ins().call(cmp_ref, &[op_v, lt, ld, rt, rd]);
    let data = b.inst_results(call)[0];
    b.ins().jump(merge_bb, &[BlockArg::Value(data)]);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    let data = b.block_params(merge_bb)[0];
    let btag = b.ins().iconst(types::I64, TAG_BOOL as i64);
    Some((btag, data))
}

/// Return `(tag, data)` from the compiled function immediately if it is a
/// runtime error, so nothing after the failing operation runs.
pub(super) fn abort_on_error(
    (tag, data): (Value, Value),
    b: &mut FunctionBuilder<'_>,
) -> (Value, Value) {
    let abort_bb = b.create_block();
    let cont_bb = b.create_block();
    let is_err = tag_is(tag, TAG_ERROR, b);
    b.ins().brif(
        is_err,
        abort_bb,
        &[] as &[BlockArg],
        cont_bb,
        &[] as &[BlockArg],
    );
    b.switch_to_block(abort_bb);
    b.seal_block(abort_bb);
    b.ins().return_(&[tag, data]);
    b.switch_to_block(cont_bb);
    b.seal_block(cont_bb);
    (tag, data)
}

pub(super) fn emit_icmp_eq(
    (at, ad): (Value, Value),
    (bt, bd): (Value, Value),
    value_eq: FuncId,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> Option<(Value, Value)> {
    let host_bb = b.create_block();
    let fast_bb = b.create_block();
    let merge_bb = b.create_block();
    b.append_block_param(merge_bb, types::I64);

    let tags_eq = b.ins().icmp(IntCC::Equal, at, bt);
    let a_int = tag_is_int(at, b);
    let b_int = tag_is_int(bt, b);
    let both_numeric = b.ins().band(a_int, b_int);
    let use_fast = b.ins().band(tags_eq, both_numeric);
    b.ins().brif(
        use_fast,
        fast_bb,
        &[] as &[BlockArg],
        host_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(fast_bb);
    b.seal_block(fast_bb);
    let cmp = b.ins().icmp(IntCC::Equal, ad, bd);
    let data = b.ins().uextend(types::I64, cmp);
    b.ins().jump(merge_bb, &[BlockArg::Value(data)]);

    b.switch_to_block(host_bb);
    b.seal_block(host_bb);
    let eq_ref = module.declare_func_in_func(value_eq, b.func);
    let call = b.ins().call(eq_ref, &[at, ad, bt, bd]);
    let data = b.inst_results(call)[0];
    b.ins().jump(merge_bb, &[BlockArg::Value(data)]);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    let tag = b.ins().iconst(types::I64, 3);
    Some((tag, b.block_params(merge_bb)[0]))
}

pub(super) fn emit_icmp_ne(
    (at, ad): (Value, Value),
    (bt, bd): (Value, Value),
    value_eq: FuncId,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> Option<(Value, Value)> {
    let pair = emit_icmp_eq((at, ad), (bt, bd), value_eq, module, b)?;
    let zero = b.ins().iconst(types::I64, 0);
    let inv = b.ins().icmp(IntCC::Equal, pair.1, zero);
    let data = b.ins().uextend(types::I64, inv);
    Some((pair.0, data))
}
