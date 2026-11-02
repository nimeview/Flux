//! Stack-machine expression evaluation and function dispatch (including the
//! higher-order builtins `map` / `filter` / `reduce` that call back into user code).

use flux_ir::Inst;

use crate::graph::RuntimeValue;
use crate::vm::{
    debug_assert_vm, is_bound, resolve_load, stack_pop, stack_pop_opt, stack_pop_opt_with,
    stack_pop_with,
};

use flux_value::NumOp;

use super::values::{
    arith_binop, cmp, eq_binop, extract_variant_payload, to_runtime, variant_tag_eq,
};
use super::{Executor, FlowControl, Locals};

impl Executor {
    pub(super) fn eval_expr(&mut self, code: &[Inst], locals: &mut Locals) -> RuntimeValue {
        // Reuse an operand stack: allocating one per expression dominated
        // tight loops.
        let mut stack = self.stack_pool.pop().unwrap_or_default();
        let value = self.eval_on_stack(code, locals, &mut stack);
        stack.clear();
        self.stack_pool.push(stack);
        value
    }

    fn eval_on_stack(
        &mut self,
        code: &[Inst],
        locals: &mut Locals,
        stack: &mut Vec<RuntimeValue>,
    ) -> RuntimeValue {
        for inst in code {
            // A runtime error aborts evaluation; nested evaluations (calls,
            // short-circuit operands) surface here on the next instruction.
            if self.trap.is_some() || self.pending_flow.is_some() {
                return RuntimeValue::Unit;
            }
            match inst {
                Inst::Const(v) => stack.push(to_runtime(v)),
                Inst::Load(name) => match locals.get(name) {
                    // Locals are never reactive dependencies.
                    Some(v) => stack.push(v.clone()),
                    None => {
                        self.record_dep_load(name, locals);
                        if !is_bound(name, locals, &self.graph, &self.functions) {
                            return self.unbound(name);
                        }
                        stack.push(resolve_load(name, locals, &self.graph, &self.functions));
                    }
                },
                Inst::Block(stmts) => {
                    let (value, flow) = self.run_stmts_flow(stmts, locals);
                    if self.trap.is_some() {
                        return RuntimeValue::Unit;
                    }
                    if flow != FlowControl::None {
                        // Unwinds to the statement that holds this expression.
                        self.pending_flow = Some((value, flow));
                        return RuntimeValue::Unit;
                    }
                }
                Inst::Index => {
                    let index = stack_pop(stack, "Index index");
                    let base = stack_pop(stack, "Index array");
                    if let RuntimeValue::Map(entries) = &base {
                        let found = flux_value::MapKey::of(&index)
                            .and_then(|key| entries.get(&key))
                            .map(|(_, v)| v.clone());
                        match found {
                            Some(v) => stack.push(v),
                            None => {
                                self.raise(flux_value::ArithError::KeyNotFound);
                                return RuntimeValue::Unit;
                            }
                        }
                        continue;
                    }
                    let element = match (base, index.as_int()) {
                        (RuntimeValue::Array(items), Some(i)) => {
                            usize::try_from(i).ok().and_then(|i| items.get(i).cloned())
                        }
                        _ => None,
                    };
                    match element {
                        Some(v) => stack.push(v),
                        None => {
                            self.raise(flux_value::ArithError::IndexOutOfBounds);
                            return RuntimeValue::Unit;
                        }
                    }
                }
                Inst::Try => match stack_pop(stack, "Try") {
                    RuntimeValue::Result(Ok(v)) | RuntimeValue::Option(Some(v)) => stack.push(*v),
                    // `Err(e)` / `None` is the function's result.
                    failed => {
                        self.pending_flow = Some((failed, FlowControl::Return));
                        return RuntimeValue::Unit;
                    }
                },
                Inst::Take(name) => match locals.remove(name) {
                    Some(v) => stack.push(v),
                    None if !is_bound(name, locals, &self.graph, &self.functions) => {
                        return self.unbound(name);
                    }
                    None => stack.push(resolve_load(name, locals, &self.graph, &self.functions)),
                },
                Inst::Add | Inst::Sub | Inst::Mul | Inst::Div | Inst::Rem => {
                    let op = match inst {
                        Inst::Add => NumOp::Add,
                        Inst::Sub => NumOp::Sub,
                        Inst::Mul => NumOp::Mul,
                        Inst::Div => NumOp::Div,
                        _ => NumOp::Rem,
                    };
                    if let Err(e) = arith_binop(stack, op) {
                        self.raise(e);
                        return RuntimeValue::Unit;
                    }
                }
                Inst::Neg => {
                    let v = stack_pop(stack, "Neg");
                    match flux_value::negate(&v) {
                        Ok(n) => stack.push(n),
                        Err(e) => {
                            self.raise(e);
                            return RuntimeValue::Unit;
                        }
                    }
                }
                Inst::Not => {
                    let v = stack_pop(stack, "Not");
                    if let Some(b) = v.as_bool() {
                        stack.push(RuntimeValue::Bool(!b));
                    } else {
                        debug_assert_vm(false, &format!("Not: expected bool value, got {v:?}"));
                    }
                }
                Inst::CmpEq => eq_binop(stack, false),
                Inst::CmpNe => eq_binop(stack, true),
                Inst::CmpLt => cmp(stack, |o| o.is_lt()),
                Inst::CmpLe => cmp(stack, |o| o.is_le()),
                Inst::CmpGt => cmp(stack, |o| o.is_gt()),
                Inst::CmpGe => cmp(stack, |o| o.is_ge()),
                Inst::ShortAnd(right) => {
                    let a = stack_pop(stack, "ShortAnd lhs")
                        .as_bool()
                        .unwrap_or_else(|| {
                            debug_assert_vm(false, "ShortAnd: expected bool on stack");
                            false
                        });
                    if a {
                        stack.push(self.eval_expr(right, locals));
                    } else {
                        stack.push(RuntimeValue::Bool(false));
                    }
                }
                Inst::ShortOr(right) => {
                    let a = stack_pop(stack, "ShortOr lhs")
                        .as_bool()
                        .unwrap_or_else(|| {
                            debug_assert_vm(false, "ShortOr: expected bool on stack");
                            false
                        });
                    if a {
                        stack.push(RuntimeValue::Bool(true));
                    } else {
                        stack.push(self.eval_expr(right, locals));
                    }
                }
                Inst::LazyIf {
                    then_branch,
                    else_branch,
                } => {
                    let cond = stack_pop(stack, "LazyIf cond")
                        .as_bool()
                        .unwrap_or_else(|| {
                            debug_assert_vm(false, "LazyIf: expected bool condition");
                            false
                        });
                    if cond {
                        stack.push(self.eval_expr(then_branch, locals));
                    } else {
                        stack.push(self.eval_expr(else_branch, locals));
                    }
                }
                Inst::Select => {
                    let else_v = stack_pop_opt(stack, "Select else");
                    let then_v = stack_pop_opt(stack, "Select then");
                    let cond = stack_pop(stack, "Select cond")
                        .as_bool()
                        .unwrap_or_else(|| {
                            debug_assert_vm(false, "Select: expected bool condition");
                            false
                        });
                    let picked = if cond { then_v } else { else_v };
                    if let Some(v) = picked {
                        stack.push(v);
                    } else {
                        debug_assert_vm(false, "Select: missing branch value on stack");
                    }
                }
                Inst::Call { name, argc } => {
                    debug_assert_vm(stack.len() >= *argc, "stack underflow in Call arguments");
                    let mut args = stack.split_off(stack.len().saturating_sub(*argc));
                    // Builtin `push` on an array: append in place. `make_mut`
                    // copies only if the array is still shared, so value
                    // semantics are unchanged.
                    if name == "push" && args.len() == 2 && !self.functions.contains_key(name) {
                        if let [RuntimeValue::Array(_), _] = args.as_slice() {
                            let value = args.pop().unwrap_or(RuntimeValue::Unit);
                            if let Some(RuntimeValue::Array(mut items)) = args.pop() {
                                if items.len() as u64 >= flux_value::MAX_COLLECTION_LEN {
                                    self.raise(flux_value::ArithError::AllocationLimit);
                                    return RuntimeValue::Unit;
                                }
                                std::sync::Arc::make_mut(&mut items).push(value);
                                stack.push(RuntimeValue::Array(items));
                            }
                            continue;
                        }
                    }
                    // Builtin `set` on an array: replace in place (copy only if
                    // shared), like `push`.
                    if name == "set" && args.len() == 3 && !self.functions.contains_key(name) {
                        if let [RuntimeValue::Map(_), _, _] = args.as_slice() {
                            let value = args.pop().unwrap_or(RuntimeValue::Unit);
                            let key_value = args.pop().unwrap_or(RuntimeValue::Unit);
                            if let (Some(RuntimeValue::Map(mut entries)), Some(key)) =
                                (args.pop(), flux_value::MapKey::of(&key_value))
                            {
                                if !entries.contains_key(&key)
                                    && entries.len() as u64 >= flux_value::MAX_COLLECTION_LEN
                                {
                                    self.raise(flux_value::ArithError::AllocationLimit);
                                    return RuntimeValue::Unit;
                                }
                                std::sync::Arc::make_mut(&mut entries)
                                    .insert(key, (key_value, value));
                                stack.push(RuntimeValue::Map(entries));
                            }
                            continue;
                        }
                        if let [RuntimeValue::Array(_), _, _] = args.as_slice() {
                            let value = args.pop().unwrap_or(RuntimeValue::Unit);
                            let index = args.pop().and_then(|i| i.as_int());
                            if let Some(RuntimeValue::Array(mut items)) = args.pop() {
                                let Some(i) = index
                                    .and_then(|i| usize::try_from(i).ok())
                                    .filter(|&i| i < items.len())
                                else {
                                    self.raise(flux_value::ArithError::IndexOutOfBounds);
                                    return RuntimeValue::Unit;
                                };
                                std::sync::Arc::make_mut(&mut items)[i] = value;
                                stack.push(RuntimeValue::Array(items));
                            }
                            continue;
                        }
                    }
                    let result = self.call_function(name, &args);
                    stack.push(result);
                }
                Inst::Cast { ty } => {
                    let val = stack_pop_with(stack, || format!("Cast as {ty}"));
                    match flux_value::cast(&val, ty) {
                        Ok(v) => stack.push(v),
                        Err(e) => {
                            self.raise(e);
                            return RuntimeValue::Unit;
                        }
                    }
                }
                Inst::MakeArray(count) => {
                    let mut items = Vec::with_capacity(*count);
                    for i in 0..*count {
                        items.push(stack_pop_with(stack, || format!("MakeArray element {i}")));
                    }
                    items.reverse();
                    stack.push(RuntimeValue::array(items));
                }
                Inst::EnumTagEq(variant) => {
                    let val = stack_pop_opt_with(stack, || format!("EnumTagEq `{variant}`"));
                    let matches = variant_tag_eq(val.as_ref(), variant);
                    stack.push(RuntimeValue::Bool(matches));
                }
                Inst::VariantTagEq(variant) => {
                    let val = stack_pop_opt_with(stack, || format!("VariantTagEq `{variant}`"));
                    let matches = variant_tag_eq(val.as_ref(), variant);
                    stack.push(RuntimeValue::Bool(matches));
                }
                Inst::VariantPayloadGet { field } => {
                    let val = stack_pop_opt(stack, "VariantPayloadGet");
                    let extracted = extract_variant_payload(val.as_ref(), field.as_deref());
                    stack.push(extracted);
                }
                Inst::BindLocal(name) => {
                    let val = stack_pop_with(stack, || format!("BindLocal `{name}`"));
                    locals.bind(name, val);
                }
                Inst::MakeUserEnum {
                    type_name,
                    variant,
                    has_payload,
                } => {
                    let payload = if *has_payload { stack.pop() } else { None };
                    stack.push(RuntimeValue::user_enum(
                        type_name.clone(),
                        variant.clone(),
                        payload,
                    ));
                }
                Inst::MakeUserStruct { type_name, fields } => {
                    let mut map = std::collections::HashMap::new();
                    for field_name in fields.iter().rev() {
                        let val = stack_pop_with(stack, || {
                            format!("MakeUserStruct `{type_name}` field `{field_name}`")
                        });
                        map.insert(field_name.clone(), val);
                    }
                    stack.push(RuntimeValue::user_struct(type_name.clone(), map));
                }
                Inst::FieldGet(field) => {
                    let val = stack_pop_with(stack, || format!("FieldGet `{field}`"));
                    if let RuntimeValue::UserStruct(s) = val {
                        match s.fields.get(field) {
                            Some(v) => stack.push(v.clone()),
                            None => {
                                debug_assert_vm(
                                    false,
                                    &format!("FieldGet: field `{field}` not found on struct"),
                                );
                                stack.push(RuntimeValue::Unit);
                            }
                        }
                    } else {
                        debug_assert_vm(
                            false,
                            &format!("FieldGet `{field}`: expected UserStruct, got {val:?}"),
                        );
                        stack.push(RuntimeValue::Unit);
                    }
                }
            }
        }
        match stack.pop() {
            Some(v) => v,
            None => {
                debug_assert_vm(
                    !code.is_empty(),
                    "expression bytecode finished with empty stack",
                );
                RuntimeValue::Unit
            }
        }
    }

    /// A name with no value at run time is a compiler bug: stop with an
    /// internal error rather than continue with `()`.
    fn unbound(&mut self, name: &str) -> RuntimeValue {
        debug_assert_vm(false, &format!("unbound name `{name}` at runtime"));
        self.raise(flux_value::ArithError::Internal);
        if let Some(trap) = self.trap.as_mut() {
            trap.detail
                .get_or_insert_with(|| format!("unbound name `{}`", name.replace('\0', "")));
        }
        RuntimeValue::Unit
    }

    pub(crate) fn call_function(&mut self, name: &str, args: &[RuntimeValue]) -> RuntimeValue {
        // Higher-order builtins are handled here so they can invoke user functions/closures.
        if flux_std::is_hof(name) {
            return match name {
                "map" => {
                    let arr = match args.first() {
                        Some(RuntimeValue::Array(a)) => a.clone(),
                        other => {
                            debug_assert_vm(
                                false,
                                &format!("map: expected Array as first argument, got {other:?}"),
                            );
                            return RuntimeValue::Unit;
                        }
                    };
                    let fn_name = match args.get(1) {
                        Some(RuntimeValue::String(s)) => s.clone(),
                        other => {
                            debug_assert_vm(
                                false,
                                &format!(
                                    "map: expected function name as second argument, got {other:?}"
                                ),
                            );
                            return RuntimeValue::Array(arr);
                        }
                    };
                    let mut result = Vec::with_capacity(arr.len());
                    for item in arr.iter() {
                        result.push(self.call_function(&fn_name, std::slice::from_ref(item)));
                    }
                    RuntimeValue::array(result)
                }
                "filter" => {
                    let arr = match args.first() {
                        Some(RuntimeValue::Array(a)) => a.clone(),
                        other => {
                            debug_assert_vm(
                                false,
                                &format!("filter: expected Array as first argument, got {other:?}"),
                            );
                            return RuntimeValue::Unit;
                        }
                    };
                    let fn_name = match args.get(1) {
                        Some(RuntimeValue::String(s)) => s.clone(),
                        _ => {
                            let filtered = arr
                                .iter()
                                .filter(|v| match v {
                                    RuntimeValue::Bool(b) => *b,
                                    v if v.as_int().is_some() => v.as_int().unwrap() != 0,
                                    _ => false,
                                })
                                .cloned()
                                .collect();
                            return RuntimeValue::array(filtered);
                        }
                    };
                    let mut result = Vec::new();
                    for item in arr.iter() {
                        if self
                            .call_function(&fn_name, std::slice::from_ref(item))
                            .as_bool()
                            .unwrap_or(false)
                        {
                            result.push(item.clone());
                        }
                    }
                    RuntimeValue::array(result)
                }
                "reduce" => {
                    let arr = match args.first() {
                        Some(RuntimeValue::Array(a)) => a.clone(),
                        other => {
                            debug_assert_vm(
                                false,
                                &format!("reduce: expected Array as first argument, got {other:?}"),
                            );
                            return RuntimeValue::Unit;
                        }
                    };
                    let fn_name = match args.get(2) {
                        Some(RuntimeValue::String(s)) => s.clone(),
                        _ => {
                            let init = match args.get(1) {
                                Some(v) => v.as_int().unwrap_or(0),
                                _ => 0,
                            };
                            let sum = arr
                                .iter()
                                .fold(init, |acc, v| acc + v.as_int().unwrap_or(0));
                            return RuntimeValue::int_i64(sum);
                        }
                    };
                    let init = args.get(1).cloned().unwrap_or(RuntimeValue::Unit);
                    let mut acc = init;
                    for item in arr.iter() {
                        acc = self.call_function(&fn_name, &[acc, item.clone()]);
                    }
                    acc
                }
                _ => RuntimeValue::Unit,
            };
        }

        if flux_std::is_builtin_fn(name) {
            match flux_std::call_builtin_checked(name, args) {
                Some(Ok(result)) => return result,
                Some(Err(e)) => {
                    self.raise(e);
                    return RuntimeValue::Unit;
                }
                None => {}
            }
            debug_assert_vm(
                false,
                &format!("builtin `{name}` returned None (invalid arguments?)"),
            );
            return RuntimeValue::Unit;
        }
        let Some(func) = self.functions.get(name) else {
            debug_assert_vm(
                false,
                &format!("call to undefined function `{name}` at runtime"),
            );
            return RuntimeValue::Unit;
        };
        // Deep recursion is a language error, not a native stack overflow.
        if self.call_depth >= flux_value::MAX_CALL_DEPTH {
            self.raise(flux_value::ArithError::CallDepthExceeded);
            return RuntimeValue::Unit;
        }
        let func = std::sync::Arc::clone(func);
        self.call_depth += 1;
        // An interpreted call takes a few KB of native stack; grow it on the
        // heap when it runs low, so the depth limit holds on any thread.
        let result = stacker::maybe_grow(64 * 1024, 2 * 1024 * 1024, || {
            self.run_user_fn(name, &func, args)
        });
        self.call_depth -= 1;
        result
    }

    fn run_user_fn(
        &mut self,
        name: &str,
        func: &flux_ir::CompiledFunction,
        args: &[RuntimeValue],
    ) -> RuntimeValue {
        match self
            .backend
            .try_call_fn(name, args, &self.graph, self.call_depth)
        {
            Some(Ok(result)) => return result,
            Some(Err(e)) => {
                self.raise(e);
                return RuntimeValue::Unit;
            }
            None => {}
        }
        let mut locals = self.locals_pool.pop().unwrap_or_default();
        for (param, arg) in func.params.iter().zip(args.iter()) {
            locals.bind(param, arg.clone());
        }
        let (value, _) = self.run_stmts_flow(&func.body, &mut locals);
        locals.unbind_all();
        if self.locals_pool.len() < super::LOCALS_POOL_LIMIT {
            self.locals_pool.push(locals);
        }
        value
    }
}
