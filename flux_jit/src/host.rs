//! JIT host imports — thin `extern "C"` shims over `flux_value` heap stores.

use std::sync::{Mutex, OnceLock};

use flux_value::{
    array_finish, array_get, array_len, array_push, get_enum, get_string, intern_string,
    make_int_range_array, store_enum, tag_kind, tagged_to_value, value_to_tagged, TaggedVal,
    TAG_ENUM, TAG_FLOAT, TAG_INT, TAG_STRING,
};

type EmitHook = Box<dyn Fn(u64, TaggedVal) + Send>;

#[derive(Default)]
struct EmitState {
    queue: Vec<(u64, TaggedVal)>,
    hook: Option<EmitHook>,
}

fn emit_state() -> &'static Mutex<EmitState> {
    static STATE: OnceLock<Mutex<EmitState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(EmitState::default()))
}

pub fn set_emit_hook(hook: Option<EmitHook>) {
    emit_state()
        .lock()
        .expect("flux_jit emit mutex poisoned")
        .hook = hook;
}

pub fn take_emit_events() -> Vec<(u64, TaggedVal)> {
    std::mem::take(
        &mut emit_state()
            .lock()
            .expect("flux_jit emit mutex poisoned")
            .queue,
    )
}

pub fn clear_stores_for_tests() {
    flux_value::clear_for_tests();
    let mut guard = emit_state().lock().expect("flux_jit emit mutex poisoned");
    guard.queue.clear();
    guard.hook = None;
}

#[no_mangle]
pub extern "C" fn flux_jit_array_push(tag: u64, data: u64) {
    array_push(tag, data);
}

#[no_mangle]
pub extern "C" fn flux_jit_array_finish(count: u64) -> TaggedVal {
    array_finish(count)
}

#[no_mangle]
pub extern "C" fn flux_jit_string_concat(
    tag_a: u64,
    data_a: u64,
    tag_b: u64,
    data_b: u64,
) -> TaggedVal {
    let a = if tag_a == TAG_STRING {
        get_string(data_a).unwrap_or_default()
    } else {
        String::new()
    };
    let b = if tag_b == TAG_STRING {
        get_string(data_b).unwrap_or_default()
    } else {
        String::new()
    };
    TaggedVal::from_string_id(intern_string(&(a + &b)))
}

#[no_mangle]
pub extern "C" fn flux_jit_value_eq(tag_a: u64, data_a: u64, tag_b: u64, data_b: u64) -> u64 {
    let numeric = |tag| matches!(tag_kind(tag), TAG_INT | TAG_FLOAT);
    if numeric(tag_a) && numeric(tag_b) {
        let a = tagged_to_value(TaggedVal {
            tag: tag_a,
            data: data_a,
        });
        let b = tagged_to_value(TaggedVal {
            tag: tag_b,
            data: data_b,
        });
        return flux_value::values_equal(&a, &b) as u64;
    }
    if tag_a != tag_b {
        return 0;
    }
    match tag_a {
        TAG_STRING => (get_string(data_a) == get_string(data_b)) as u64,
        TAG_ENUM => match (get_enum(data_a), get_enum(data_b)) {
            (Some((ta, va, pa)), Some((tb, vb, pb))) => (ta == tb && va == vb && pa == pb) as u64,
            _ => 0,
        },
        _ => (data_a == data_b) as u64,
    }
}

#[no_mangle]
pub extern "C" fn flux_jit_make_enum(
    type_name_id: u64,
    variant_name_id: u64,
    has_payload: u64,
    payload_tag: u64,
    payload_data: u64,
) -> TaggedVal {
    let type_name = get_string(type_name_id).unwrap_or_default();
    let variant = get_string(variant_name_id).unwrap_or_default();
    let payload = if has_payload != 0 {
        Some(TaggedVal {
            tag: payload_tag,
            data: payload_data,
        })
    } else {
        None
    };
    TaggedVal::from_enum_id(store_enum(&type_name, &variant, payload))
}

#[no_mangle]
pub extern "C" fn flux_jit_enum_variant_eq(enum_id: u64, variant_name_id: u64) -> u64 {
    let want = match get_string(variant_name_id) {
        Some(s) => s,
        None => return 0,
    };
    get_enum(enum_id)
        .map(|(_, v, _)| (v == want) as u64)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn flux_jit_enum_payload_get(enum_id: u64) -> TaggedVal {
    get_enum(enum_id)
        .and_then(|(_, _, p)| p)
        .unwrap_or(TaggedVal::UNIT)
}

#[no_mangle]
pub extern "C" fn flux_jit_emit(event_idx: u64, tag: u64, data: u64) {
    let tv = TaggedVal { tag, data };
    let mut guard = emit_state().lock().expect("flux_jit emit mutex poisoned");
    guard.queue.push((event_idx, tv));
    if let Some(ref f) = guard.hook {
        f(event_idx, tv);
    }
}

#[no_mangle]
pub extern "C" fn flux_jit_array_len(array_id: u64) -> u64 {
    array_len(array_id)
}

#[no_mangle]
pub extern "C" fn flux_jit_array_get(array_id: u64, index: u64) -> TaggedVal {
    array_get(array_id, index)
}

/// `argc`: 1 = range(end), 2 = range(start,end), 3 = range(start,end,step)
#[no_mangle]
pub extern "C" fn flux_jit_range(a: u64, b: u64, c: u64, argc: u64) -> TaggedVal {
    let (start, end, step) = match argc {
        1 => (0, a as i64, 1),
        2 => (a as i64, b as i64, 1),
        _ => (a as i64, b as i64, c as i64),
    };
    match make_int_range_array(start, end, step) {
        Ok(id) => TaggedVal::from_array_id(id),
        Err(e) => TaggedVal::error(e),
    }
}

/// Call a pure builtin from JIT-compiled code. Args are read from `slots` at indices
/// `base..base+argc` as (tag, data) pairs laid out contiguously (tag, data, tag, data, …).
#[no_mangle]
pub extern "C" fn flux_jit_call_builtin(
    name_id: u64,
    slots: *const u64,
    base: u64,
    argc: u64,
) -> TaggedVal {
    let name = match get_string(name_id) {
        Some(s) => s,
        None => return TaggedVal::UNIT,
    };
    let mut args = Vec::with_capacity(argc as usize);
    if !slots.is_null() {
        for i in 0..argc as usize {
            let pair = base as usize + i;
            let tag = unsafe { *slots.add(pair * 2) };
            let data = unsafe { *slots.add(pair * 2 + 1) };
            args.push(tagged_to_value(TaggedVal { tag, data }));
        }
    }
    match flux_std::call_builtin_checked(&name, &args) {
        Some(Ok(v)) => value_to_tagged(&v),
        Some(Err(e)) => TaggedVal::error(e),
        None => TaggedVal::UNIT,
    }
}

#[no_mangle]
pub extern "C" fn flux_jit_cast(tag: u64, data: u64, ty_id: u64) -> TaggedVal {
    let ty = get_string(ty_id).unwrap_or_default();
    let v = tagged_to_value(TaggedVal { tag, data });
    match flux_value::cast(&v, &ty) {
        Ok(v) => value_to_tagged(&v),
        Err(e) => TaggedVal::error(e),
    }
}

/// Slow path for `+ - * / %`: every operand mix outside the inline
/// `Int64`/`Float64` fast path, and every fast-path overflow or zero divisor.
/// Uses the VM's `flux_value::binary`, so both engines agree by construction.
#[no_mangle]
pub extern "C" fn flux_jit_arith(
    op: u64,
    tag_a: u64,
    data_a: u64,
    tag_b: u64,
    data_b: u64,
) -> TaggedVal {
    let Some(op) = flux_value::NumOp::from_code(op) else {
        return TaggedVal::error(flux_value::ArithError::InvalidOperands);
    };
    if op == flux_value::NumOp::Add && tag_a == TAG_STRING && tag_b == TAG_STRING {
        return flux_jit_string_concat(tag_a, data_a, tag_b, data_b);
    }
    let a = tagged_to_value(TaggedVal {
        tag: tag_a,
        data: data_a,
    });
    let b = tagged_to_value(TaggedVal {
        tag: tag_b,
        data: data_b,
    });
    match flux_value::binary(op, &a, &b) {
        Ok(v) => value_to_tagged(&v),
        Err(e) => TaggedVal::error(e),
    }
}

#[no_mangle]
pub extern "C" fn flux_jit_neg(tag: u64, data: u64) -> TaggedVal {
    match flux_value::negate(&tagged_to_value(TaggedVal { tag, data })) {
        Ok(v) => value_to_tagged(&v),
        Err(e) => TaggedVal::error(e),
    }
}

/// Ordered comparison slow path. `op`: 0 = `<`, 1 = `<=`, 2 = `>`, 3 = `>=`.
#[no_mangle]
pub extern "C" fn flux_jit_cmp(op: u64, tag_a: u64, data_a: u64, tag_b: u64, data_b: u64) -> u64 {
    let a = tagged_to_value(TaggedVal {
        tag: tag_a,
        data: data_a,
    });
    let b = tagged_to_value(TaggedVal {
        tag: tag_b,
        data: data_b,
    });
    flux_value::compare(&a, &b).is_some_and(|o| match op {
        0 => o.is_lt(),
        1 => o.is_le(),
        2 => o.is_gt(),
        _ => o.is_ge(),
    }) as u64
}
