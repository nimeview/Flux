use std::collections::HashMap;

use crate::hir::{Expr, Stmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Number,
    Bool,
    VectorNumber,
}

pub fn type_check(program: &[Stmt]) -> Result<(), String> {
    let mut env: HashMap<String, Type> = HashMap::new();

    for stmt in program {
        match stmt {
            Stmt::Signal(name, expr) | Stmt::Reactive(name, expr) => {
                let ty = infer_expr(expr, &env)?;
                env.insert(name.clone(), ty);
            }
            Stmt::Set(name, expr) => {
                let current = env
                    .get(name)
                    .ok_or_else(|| format!("cannot set unknown name `{name}`"))?
                    .clone();
                let next = infer_expr(expr, &env)?;
                if current != next {
                    return Err(format!("type mismatch in set `{name}`: expected {current:?}, got {next:?}"));
                }
            }
            Stmt::Expr(expr) => {
                let _ = infer_expr(expr, &env)?;
            }
        }
    }

    Ok(())
}

fn infer_expr(expr: &Expr, env: &HashMap<String, Type>) -> Result<Type, String> {
    match expr {
        Expr::Number(_) => Ok(Type::Number),
        Expr::Bool(_) => Ok(Type::Bool),
        Expr::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown name `{name}`")),
        Expr::Binary(l, _op, r) => {
            let lt = infer_expr(l, env)?;
            let rt = infer_expr(r, env)?;
            match (lt, rt) {
                (Type::Number, Type::Number) => Ok(Type::Number),
                (a, b) => Err(format!("binary op expects numbers, got {a:?} and {b:?}")),
            }
        }
        Expr::Call { name, args } => infer_call(name, args, env),
    }
}

fn infer_call(name: &str, args: &[Expr], env: &HashMap<String, Type>) -> Result<Type, String> {
    let arg_types: Vec<Type> = args
        .iter()
        .map(|a| infer_expr(a, env))
        .collect::<Result<Vec<_>, _>>()?;

    match name {
        "print" => {
            if arg_types.len() != 1 { return Err("print expects 1 argument".to_string()); }
            Ok(arg_types[0].clone())
        }
        "sqrt" => expect_exact(name, &arg_types, &[Type::Number]).map(|_| Type::Number),
        "min" | "max" => expect_exact(name, &arg_types, &[Type::Number, Type::Number]).map(|_| Type::Number),
        "now_ms" => expect_exact(name, &arg_types, &[]).map(|_| Type::Number),
        "vec" => {
            for t in &arg_types {
                if *t != Type::Number { return Err("vec expects number arguments".to_string()); }
            }
            Ok(Type::VectorNumber)
        }
        "len" => expect_exact(name, &arg_types, &[Type::VectorNumber]).map(|_| Type::Number),
        _ => Err(format!("unknown function `{name}`")),
    }
}

fn expect_exact(name: &str, got: &[Type], expected: &[Type]) -> Result<(), String> {
    if got == expected { return Ok(()); }
    Err(format!("function `{name}` argument mismatch: expected {expected:?}, got {got:?}"))
}
