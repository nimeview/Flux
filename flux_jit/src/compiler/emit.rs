use cranelift::codegen::ir::{BlockArg, Endianness};
use cranelift::prelude::*;
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};
use flux_ir::IrValue;

use super::tables::JitTables;
use flux_value::{TAG_BOOL, TAG_STRING};

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
            let tag = b.ins().iconst(types::I64, 1);
            let data = b.ins().iconst(types::I64, *n);
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

fn both_int(lt: Value, rt: Value, b: &mut FunctionBuilder<'_>) -> Value {
    let li = tag_is_int(lt, b);
    let ri = tag_is_int(rt, b);
    b.ins().band(li, ri)
}

macro_rules! emit_numeric_binop {
    ($name:ident, $int_op:ident, $float_op:ident) => {
        pub(super) fn $name(
            (lt, ld): (Value, Value),
            (rt, rd): (Value, Value),
            b: &mut FunctionBuilder<'_>,
        ) -> (Value, Value) {
            let int_bb = b.create_block();
            let float_bb = b.create_block();
            let merge_bb = b.create_block();
            b.append_block_param(merge_bb, types::I64);
            b.append_block_param(merge_bb, types::I64);

            let is_int = both_int(lt, rt, b);
            b.ins().brif(
                is_int,
                int_bb,
                &[] as &[BlockArg],
                float_bb,
                &[] as &[BlockArg],
            );

            b.switch_to_block(int_bb);
            b.seal_block(int_bb);
            let r = b.ins().$int_op(ld, rd);
            let itag = b.ins().iconst(types::I64, 1);
            b.ins()
                .jump(merge_bb, &[BlockArg::Value(itag), BlockArg::Value(r)]);

            b.switch_to_block(float_bb);
            b.seal_block(float_bb);
            let af = b.ins().bitcast(types::F64, float_bits_flags(), ld);
            let bf = b.ins().bitcast(types::F64, float_bits_flags(), rd);
            let rf = b.ins().$float_op(af, bf);
            let bits = b.ins().bitcast(types::I64, float_bits_flags(), rf);
            let ftag = b.ins().iconst(types::I64, 2);
            b.ins()
                .jump(merge_bb, &[BlockArg::Value(ftag), BlockArg::Value(bits)]);

            b.switch_to_block(merge_bb);
            b.seal_block(merge_bb);
            (b.block_params(merge_bb)[0], b.block_params(merge_bb)[1])
        }
    };
}

fn both_string(lt: Value, rt: Value, b: &mut FunctionBuilder<'_>) -> Value {
    let st = b.ins().iconst(types::I64, TAG_STRING as i64);
    let ls = b.ins().icmp(IntCC::Equal, lt, st);
    let rs = b.ins().icmp(IntCC::Equal, rt, st);
    b.ins().band(ls, rs)
}

pub(super) fn emit_add(
    (lt, ld): (Value, Value),
    (rt, rd): (Value, Value),
    string_concat: FuncId,
    module: &mut JITModule,
    b: &mut FunctionBuilder<'_>,
) -> (Value, Value) {
    let str_bb = b.create_block();
    let num_bb = b.create_block();
    let int_bb = b.create_block();
    let float_bb = b.create_block();
    let merge_bb = b.create_block();
    b.append_block_param(merge_bb, types::I64);
    b.append_block_param(merge_bb, types::I64);

    let is_str = both_string(lt, rt, b);
    b.ins().brif(
        is_str,
        str_bb,
        &[] as &[BlockArg],
        num_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(str_bb);
    b.seal_block(str_bb);
    let concat_ref = module.declare_func_in_func(string_concat, b.func);
    let call = b.ins().call(concat_ref, &[lt, ld, rt, rd]);
    let results = b.inst_results(call);
    let rt_out = results[0];
    let rd_out = results[1];
    b.ins().jump(
        merge_bb,
        &[BlockArg::Value(rt_out), BlockArg::Value(rd_out)],
    );

    b.switch_to_block(num_bb);
    b.seal_block(num_bb);
    let is_int = both_int(lt, rt, b);
    b.ins().brif(
        is_int,
        int_bb,
        &[] as &[BlockArg],
        float_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(int_bb);
    b.seal_block(int_bb);
    let r = b.ins().iadd(ld, rd);
    let itag = b.ins().iconst(types::I64, 1);
    b.ins()
        .jump(merge_bb, &[BlockArg::Value(itag), BlockArg::Value(r)]);

    b.switch_to_block(float_bb);
    b.seal_block(float_bb);
    let af = b.ins().bitcast(types::F64, float_bits_flags(), ld);
    let bf = b.ins().bitcast(types::F64, float_bits_flags(), rd);
    let rf = b.ins().fadd(af, bf);
    let bits = b.ins().bitcast(types::I64, float_bits_flags(), rf);
    let ftag = b.ins().iconst(types::I64, 2);
    b.ins()
        .jump(merge_bb, &[BlockArg::Value(ftag), BlockArg::Value(bits)]);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    (b.block_params(merge_bb)[0], b.block_params(merge_bb)[1])
}
emit_numeric_binop!(emit_sub, isub, fsub);
emit_numeric_binop!(emit_mul, imul, fmul);

fn promote_to_float(tag: Value, data: Value, b: &mut FunctionBuilder<'_>) -> Value {
    let int_bb = b.create_block();
    let float_bb = b.create_block();
    let merge_bb = b.create_block();
    b.append_block_param(merge_bb, types::F64);

    let is_int = tag_is_int(tag, b);
    b.ins().brif(
        is_int,
        int_bb,
        &[] as &[BlockArg],
        float_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(int_bb);
    b.seal_block(int_bb);
    let fv = b.ins().fcvt_from_sint(types::F64, data);
    b.ins().jump(merge_bb, &[BlockArg::Value(fv)]);

    b.switch_to_block(float_bb);
    b.seal_block(float_bb);
    let fv = b.ins().bitcast(types::F64, float_bits_flags(), data);
    b.ins().jump(merge_bb, &[BlockArg::Value(fv)]);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    b.block_params(merge_bb)[0]
}

pub(super) fn emit_div(
    (lt, ld): (Value, Value),
    (rt, rd): (Value, Value),
    b: &mut FunctionBuilder<'_>,
) -> (Value, Value) {
    // Match VM: division always promotes operands to float.
    let af = promote_to_float(lt, ld, b);
    let bf = promote_to_float(rt, rd, b);
    let rf = b.ins().fdiv(af, bf);
    let bits = b.ins().bitcast(types::I64, float_bits_flags(), rf);
    let ftag = b.ins().iconst(types::I64, 2);
    (ftag, bits)
}

pub(super) fn emit_neg((tag, data): (Value, Value), b: &mut FunctionBuilder<'_>) -> (Value, Value) {
    let int_bb = b.create_block();
    let float_bb = b.create_block();
    let merge_bb = b.create_block();
    b.append_block_param(merge_bb, types::I64);
    b.append_block_param(merge_bb, types::I64);

    let is_int = tag_is_int(tag, b);
    b.ins().brif(
        is_int,
        int_bb,
        &[] as &[BlockArg],
        float_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(int_bb);
    b.seal_block(int_bb);
    let zero = b.ins().iconst(types::I64, 0);
    let r = b.ins().isub(zero, data);
    let itag = b.ins().iconst(types::I64, 1);
    b.ins()
        .jump(merge_bb, &[BlockArg::Value(itag), BlockArg::Value(r)]);

    b.switch_to_block(float_bb);
    b.seal_block(float_bb);
    let af = b.ins().bitcast(types::F64, float_bits_flags(), data);
    let rf = b.ins().fneg(af);
    let bits = b.ins().bitcast(types::I64, float_bits_flags(), rf);
    let ftag = b.ins().iconst(types::I64, 2);
    b.ins()
        .jump(merge_bb, &[BlockArg::Value(ftag), BlockArg::Value(bits)]);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    (b.block_params(merge_bb)[0], b.block_params(merge_bb)[1])
}

pub(super) fn emit_cmp_ordered(
    cc_int: IntCC,
    cc_float: FloatCC,
    (lt, ld): (Value, Value),
    (rt, rd): (Value, Value),
    b: &mut FunctionBuilder<'_>,
) -> Option<(Value, Value)> {
    let int_bb = b.create_block();
    let float_bb = b.create_block();
    let merge_bb = b.create_block();
    b.append_block_param(merge_bb, types::I64);

    let is_int = both_int(lt, rt, b);
    b.ins().brif(
        is_int,
        int_bb,
        &[] as &[BlockArg],
        float_bb,
        &[] as &[BlockArg],
    );

    b.switch_to_block(int_bb);
    b.seal_block(int_bb);
    let cmp = b.ins().icmp(cc_int, ld, rd);
    let data = b.ins().uextend(types::I64, cmp);
    b.ins().jump(merge_bb, &[BlockArg::Value(data)]);

    b.switch_to_block(float_bb);
    b.seal_block(float_bb);
    let af = b.ins().bitcast(types::F64, float_bits_flags(), ld);
    let bf = b.ins().bitcast(types::F64, float_bits_flags(), rd);
    let cmp = b.ins().fcmp(cc_float, af, bf);
    let data = b.ins().uextend(types::I64, cmp);
    b.ins().jump(merge_bb, &[BlockArg::Value(data)]);

    b.switch_to_block(merge_bb);
    b.seal_block(merge_bb);
    let data = b.block_params(merge_bb)[0];
    let btag = b.ins().iconst(types::I64, 3);
    Some((btag, data))
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
