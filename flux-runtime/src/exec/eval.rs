//! Stack-machine expression evaluation and function dispatch (including the
//! higher-order builtins `map` / `filter` / `reduce` that call back into user code).

use std::collections::HashMap;

use flux_ir::Inst;

use crate::graph::RuntimeValue;
use crate::vm::{debug_assert_vm, resolve_load, stack_pop, stack_pop_opt};

use flux_value::NumOp;

use super::values::{
    arith_binop, cmp, eq_binop, extract_variant_payload, to_runtime, variant_tag_eq,
};
use super::{Executor, FlowControl};

impl Executor {
    pub(super) fn eval_expr(
        &mut self,
        code: &[Inst],
        locals: &mut HashMap<String, RuntimeValue>,
    ) -> RuntimeValue {
        let mut stack: Vec<RuntimeValue> = Vec::new();
        for inst in code {
            // A runtime error aborts evaluation; nested evaluations (calls,
            // short-circuit operands) surface here on the next instruction.
            if self.trap.is_some() {
                return RuntimeValue::Unit;
            }
            match inst {
                Inst::Const(v) => stack.push(to_runtime(v)),
                Inst::Load(name) => {
                    self.record_dep_load(name, locals);
                    stack.push(resolve_load(name, locals, &self.graph, &self.functions));
                }
                Inst::Add | Inst::Sub | Inst::Mul | Inst::Div | Inst::Rem => {
                    let op = match inst {
                        Inst::Add => NumOp::Add,
                        Inst::Sub => NumOp::Sub,
                        Inst::Mul => NumOp::Mul,
                        Inst::Div => NumOp::Div,
                        _ => NumOp::Rem,
                    };
                    if let Err(e) = arith_binop(&mut stack, op) {
                        self.raise(e);
                        return RuntimeValue::Unit;
                    }
                }
                Inst::Neg => {
                    let v = stack_pop(&mut stack, "Neg");
                    match flux_value::negate(&v) {
                        Ok(n) => stack.push(n),
                        Err(e) => {
                            self.raise(e);
                            return RuntimeValue::Unit;
                        }
                    }
                }
                Inst::Not => {
                    let v = stack_pop(&mut stack, "Not");
                    if let Some(b) = v.as_bool() {
                        stack.push(RuntimeValue::Bool(!b));
                    } else {
                        debug_assert_vm(false, &format!("Not: expected bool value, got {v:?}"));
                    }
                }
                Inst::CmpEq => eq_binop(&mut stack, false),
                Inst::CmpNe => eq_binop(&mut stack, true),
                Inst::CmpLt => cmp(&mut stack, |o| o.is_lt()),
                Inst::CmpLe => cmp(&mut stack, |o| o.is_le()),
                Inst::CmpGt => cmp(&mut stack, |o| o.is_gt()),
                Inst::CmpGe => cmp(&mut stack, |o| o.is_ge()),
                Inst::ShortAnd(right) => {
                    let a = stack_pop(&mut stack, "ShortAnd lhs")
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
                    let a = stack_pop(&mut stack, "ShortOr lhs")
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
                    let cond = stack_pop(&mut stack, "LazyIf cond")
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
                    let else_v = stack_pop_opt(&mut stack, "Select else");
                    let then_v = stack_pop_opt(&mut stack, "Select then");
                    let cond = stack_pop(&mut stack, "Select cond")
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
                    let mut args = Vec::with_capacity(*argc);
                    for i in 0..*argc {
                        args.push(stack_pop(&mut stack, &format!("Call `{name}` arg {i}")));
                    }
                    args.reverse();
                    let result = self.call_function(name, &args);
                    stack.push(result);
                }
                Inst::Cast { ty } => {
                    let val = stack_pop(&mut stack, &format!("Cast as {ty}"));
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
                        items.push(stack_pop(&mut stack, &format!("MakeArray element {i}")));
                    }
                    items.reverse();
                    stack.push(RuntimeValue::Array(items));
                }
                Inst::EnumTagEq(variant) => {
                    let val = stack_pop_opt(&mut stack, &format!("EnumTagEq `{variant}`"));
                    let matches = variant_tag_eq(val.as_ref(), variant);
                    stack.push(RuntimeValue::Bool(matches));
                }
                Inst::VariantTagEq(variant) => {
                    let val = stack_pop_opt(&mut stack, &format!("VariantTagEq `{variant}`"));
                    let matches = variant_tag_eq(val.as_ref(), variant);
                    stack.push(RuntimeValue::Bool(matches));
                }
                Inst::VariantPayloadGet { field } => {
                    let val = stack_pop_opt(&mut stack, "VariantPayloadGet");
                    let extracted = extract_variant_payload(val.as_ref(), field.as_deref());
                    stack.push(extracted);
                }
                Inst::BindLocal(name) => {
                    let val = stack_pop(&mut stack, &format!("BindLocal `{name}`"));
                    locals.insert(name.clone(), val);
                }
                Inst::MakeUserEnum {
                    type_name,
                    variant,
                    has_payload,
                } => {
                    let payload = if *has_payload {
                        stack.pop().map(Box::new)
                    } else {
                        None
                    };
                    stack.push(RuntimeValue::UserEnum {
                        type_name: type_name.clone(),
                        variant: variant.clone(),
                        payload,
                    });
                }
                Inst::MakeUserStruct { type_name, fields } => {
                    let mut map = std::collections::HashMap::new();
                    for field_name in fields.iter().rev() {
                        let val = stack_pop(
                            &mut stack,
                            &format!("MakeUserStruct `{type_name}` field `{field_name}`"),
                        );
                        map.insert(field_name.clone(), val);
                    }
                    stack.push(RuntimeValue::UserStruct {
                        type_name: type_name.clone(),
                        fields: map,
                    });
                }
                Inst::FieldGet(field) => {
                    let val = stack_pop(&mut stack, &format!("FieldGet `{field}`"));
                    if let RuntimeValue::UserStruct { fields, .. } = val {
                        match fields.get(field) {
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

    fn call_function(&mut self, name: &str, args: &[RuntimeValue]) -> RuntimeValue {
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
                    for item in arr {
                        result.push(self.call_function(&fn_name, &[item]));
                    }
                    RuntimeValue::Array(result)
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
                                .into_iter()
                                .filter(|v| match v {
                                    RuntimeValue::Bool(b) => *b,
                                    v if v.as_int().is_some() => v.as_int().unwrap() != 0,
                                    _ => false,
                                })
                                .collect();
                            return RuntimeValue::Array(filtered);
                        }
                    };
                    let mut result = Vec::new();
                    for item in arr {
                        if self
                            .call_function(&fn_name, std::slice::from_ref(&item))
                            .as_bool()
                            .unwrap_or(false)
                        {
                            result.push(item);
                        }
                    }
                    RuntimeValue::Array(result)
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
                    for item in arr {
                        acc = self.call_function(&fn_name, &[acc, item]);
                    }
                    acc
                }
                _ => RuntimeValue::Unit,
            };
        }

        if flux_std::is_builtin_fn(name) {
            if let Some(result) = flux_std::call_builtin(name, args) {
                return result;
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
        match self.backend.try_call_fn(name, args, &self.graph) {
            Some(Ok(result)) => return result,
            Some(Err(e)) => {
                self.raise(e);
                return RuntimeValue::Unit;
            }
            None => {}
        }
        let func = func.clone();
        let mut locals = HashMap::new();
        for (param, arg) in func.params.iter().zip(args.iter()) {
            locals.insert(param.clone(), arg.clone());
        }
        match self.run_stmts_flow(&func.body, &mut locals) {
            (v, FlowControl::Return) | (v, FlowControl::None) => v,
            (v, _) => v,
        }
    }
}
