//! VM invariants and debug checks for stack-machine execution.
//!
//! In debug builds, invalid bytecode or stack misuse triggers `debug_assert!`.
//! Release builds keep the previous fallbacks (`Unit`, empty stack) for resilience.

use std::collections::HashSet;

use flux_ir::{CompiledFunction, CompiledProgram, Inst, IrStmt, NodeKind};

use crate::graph::{ReactiveGraph, RuntimeValue};

#[inline]
pub(crate) fn debug_assert_vm(cond: bool, msg: &str) {
    debug_assert!(cond, "flux VM invariant violated: {msg}");
}

pub(crate) fn stack_pop(stack: &mut Vec<RuntimeValue>, context: &str) -> RuntimeValue {
    if let Some(v) = stack.pop() {
        v
    } else {
        debug_assert_vm(false, &format!("stack underflow during {context}"));
        RuntimeValue::Unit
    }
}

pub(crate) fn stack_pop_opt(stack: &mut Vec<RuntimeValue>, context: &str) -> Option<RuntimeValue> {
    if let Some(v) = stack.pop() {
        Some(v)
    } else {
        debug_assert_vm(false, &format!("stack underflow during {context}"));
        None
    }
}

pub(crate) fn resolve_load(
    name: &str,
    locals: &std::collections::HashMap<String, RuntimeValue>,
    graph: &ReactiveGraph,
    functions: &std::collections::HashMap<String, CompiledFunction>,
) -> RuntimeValue {
    if let Some(v) = locals.get(name) {
        return v.clone();
    }
    if let Some(v) = graph.get_by_name(name) {
        return v.clone();
    }
    if functions.contains_key(name) || flux_std::is_builtin_fn(name) {
        return RuntimeValue::String(name.to_string());
    }
    debug_assert_vm(
        false,
        &format!("undefined variable `{name}` at runtime (compiler bug or corrupt bytecode)"),
    );
    RuntimeValue::Unit
}

pub fn validate_program(program: &CompiledProgram) -> Result<(), String> {
    let graph_names: HashSet<String> = program.nodes.iter().map(|n| n.name.clone()).collect();
    let fn_names: HashSet<String> = program.functions.iter().map(|f| f.name.clone()).collect();

    let mut global_allowed: HashSet<String> = graph_names.iter().cloned().collect();
    global_allowed.extend(fn_names.iter().cloned());

    for node in &program.nodes {
        if node.kind == NodeKind::Derived {
            validate_code(
                &node.code,
                global_allowed.clone(),
                &format!("derived binding `{}`", node.name),
            )?;
        }
    }

    validate_stmts(&program.init_stmts, global_allowed.clone(), "module init")?;

    for watch in &program.watches {
        validate_stmts(&watch.body, global_allowed.clone(), "watch body")?;
    }

    for handler in &program.on_handlers {
        let mut allowed = global_allowed.clone();
        if let Some(param) = &handler.param {
            allowed.insert(param.clone());
        }
        validate_stmts(&handler.body, allowed, "on handler")?;
    }

    for func in &program.functions {
        let mut allowed = global_allowed.clone();
        allowed.extend(func.params.iter().cloned());
        validate_stmts(&func.body, allowed, &format!("function `{}`", func.name))?;
    }
    Ok(())
}

fn validate_stmts(
    stmts: &[IrStmt],
    mut allowed: HashSet<String>,
    context: &str,
) -> Result<(), String> {
    for stmt in stmts {
        validate_stmt(stmt, &mut allowed, context)?;
    }
    Ok(())
}

fn validate_stmt(
    stmt: &IrStmt,
    allowed: &mut HashSet<String>,
    context: &str,
) -> Result<(), String> {
    match stmt {
        IrStmt::Assign { target, expr } => {
            validate_code(expr, allowed.clone(), context)?;
            allowed.insert(target.clone());
        }
        IrStmt::Let { name, expr } => {
            validate_code(expr, allowed.clone(), context)?;
            allowed.insert(name.clone());
        }
        IrStmt::Expr { code } => validate_code(code, allowed.clone(), context)?,
        IrStmt::Emit {
            arg: Some(expr), ..
        } => validate_code(expr, allowed.clone(), context)?,
        IrStmt::Emit { .. } => {}
        IrStmt::Return {
            expr: Some(expr), ..
        } => validate_code(expr, allowed.clone(), context)?,
        IrStmt::Return { .. } => {}
        IrStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            validate_code(cond, allowed.clone(), context)?;
            validate_stmts(then_body, allowed.clone(), context)?;
            if let Some(else_body) = else_body {
                validate_stmts(else_body, allowed.clone(), context)?;
            }
        }
        IrStmt::While { cond, body } => {
            validate_code(cond, allowed.clone(), context)?;
            validate_stmts(body, allowed.clone(), context)?;
        }
        IrStmt::For { var, iter, body } => {
            validate_code(iter, allowed.clone(), context)?;
            let mut loop_allowed = allowed.clone();
            loop_allowed.insert(var.clone());
            validate_stmts(body, loop_allowed, context)?;
        }
        IrStmt::Break | IrStmt::Continue => {}
    }
    Ok(())
}

fn validate_code(code: &[Inst], mut allowed: HashSet<String>, context: &str) -> Result<(), String> {
    for inst in code {
        match inst {
            Inst::Load(name) if !allowed.contains(name) && !flux_std::is_builtin_fn(name) => {
                return Err(format!("Load of undefined name `{name}` in {context}"));
            }
            Inst::Load(_) => {}
            Inst::BindLocal(name) => {
                allowed.insert(name.clone());
            }
            Inst::ShortAnd(inner) | Inst::ShortOr(inner) => {
                validate_code(inner, allowed.clone(), context)?;
            }
            Inst::LazyIf {
                then_branch,
                else_branch,
            } => {
                validate_code(then_branch, allowed.clone(), context)?;
                validate_code(else_branch, allowed.clone(), context)?;
            }
            _ => {}
        }
    }
    Ok(())
}
