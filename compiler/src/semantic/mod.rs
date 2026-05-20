use std::collections::HashMap;

use crate::hir::{Expr, Pattern, Stmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Number,
    Bool,
    String,
    Tuple(Vec<Type>),
    VectorNumber,
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Function { params: Vec<Type>, ret: Box<Type> },
    Unknown,
}

pub fn type_check(program: &[Stmt]) -> Result<(), String> {
    let mut env: HashMap<String, Type> = HashMap::new();
    let mut imports: HashMap<String, String> = HashMap::new(); // alias -> full path

    for stmt in program {
        if let Stmt::Import { path, alias } = stmt {
            let key = alias.clone().unwrap_or_else(|| path.split("::").last().unwrap_or(path).to_string());
            imports.insert(key, path.clone());
        }
    }

    for stmt in program {
        match stmt {
            Stmt::Import { .. } => {}
            Stmt::Signal(name, expr) | Stmt::Reactive(name, expr) => {
                let ty = infer_expr(expr, &env, &imports)?;
                env.insert(name.clone(), ty);
            }
            Stmt::Set(name, expr) => {
                let resolved = resolve_name(name, &imports);
                let current = env.get(&resolved).or_else(|| env.get(name)).ok_or_else(|| format!("cannot set unknown name `{name}`"))?.clone();
                let next = infer_expr(expr, &env, &imports)?;
                if !type_compatible(&current, &next) {
                    return Err(format!("type mismatch in set `{name}`: expected {current:?}, got {next:?}"));
                }
            }
            Stmt::FnDef { name, params, body } => {
                let mut local = env.clone();
                for p in params { local.insert(p.clone(), Type::Unknown); }
                let ret = infer_expr(body, &local, &imports)?;
                env.insert(name.clone(), Type::Function { params: vec![Type::Unknown; params.len()], ret: Box::new(ret) });
            }
            Stmt::Expr(expr) => { let _ = infer_expr(expr, &env, &imports)?; }
        }
    }

    Ok(())
}

fn resolve_name(name: &str, imports: &HashMap<String, String>) -> String {
    if let Some((head, tail)) = name.split_once("::") {
        if let Some(path) = imports.get(head) {
            return format!("{}::{}", path, tail);
        }
    }
    name.to_string()
}

fn infer_expr(expr: &Expr, env: &HashMap<String, Type>, imports: &HashMap<String, String>) -> Result<Type, String> {
    match expr {
        Expr::Number(_) => Ok(Type::Number),
        Expr::Bool(_) => Ok(Type::Bool),
        Expr::String(_) => Ok(Type::String),
        Expr::Tuple(items) => {
            let mut tys = Vec::with_capacity(items.len());
            for i in items { tys.push(infer_expr(i, env, imports)?); }
            Ok(Type::Tuple(tys))
        }
        Expr::Var(name) => {
            let resolved = resolve_name(name, imports);
            env.get(&resolved).or_else(|| env.get(name)).cloned().ok_or_else(|| format!("unknown name `{name}`"))
        }
        Expr::Binary(l, _op, r) => {
            let lt = infer_expr(l, env, imports)?;
            let rt = infer_expr(r, env, imports)?;
            match (lt, rt) {
                (Type::Number, Type::Number) => Ok(Type::Number),
                (a, b) => Err(format!("binary op expects numbers, got {a:?} and {b:?}")),
            }
        }
        Expr::If { cond, then_branch, else_branch } => {
            let ct = infer_expr(cond, env, imports)?;
            if ct != Type::Bool { return Err("if condition must be bool".to_string()); }
            let tt = infer_expr(then_branch, env, imports)?;
            let et = infer_expr(else_branch, env, imports)?;
            if !type_compatible(&tt, &et) { return Err(format!("if branches type mismatch: {tt:?} vs {et:?}")); }
            Ok(merge_type(tt, et))
        }
        Expr::Match { scrutinee, arms } => {
            let st = infer_expr(scrutinee, env, imports)?;
            let mut out: Option<Type> = None;
            for (pat, expr) in arms {
                let mut local = env.clone();
                bind_pattern(pat, &st, &mut local)?;
                let ty = infer_expr(expr, &local, imports)?;
                out = Some(match out {
                    None => ty,
                    Some(prev) => {
                        if !type_compatible(&prev, &ty) { return Err(format!("match arm type mismatch: {prev:?} vs {ty:?}")); }
                        merge_type(prev, ty)
                    }
                });
            }
            out.ok_or_else(|| "match must have at least one arm".to_string())
        }
        Expr::Lambda { params, body } => {
            let mut local = env.clone();
            for p in params { local.insert(p.clone(), Type::Unknown); }
            let ret = infer_expr(body, &local, imports)?;
            Ok(Type::Function { params: vec![Type::Unknown; params.len()], ret: Box::new(ret) })
        }
        Expr::Call { name, args } => infer_call(name, args, env, imports),
    }
}

fn bind_pattern(pat: &Pattern, scrutinee: &Type, env: &mut HashMap<String, Type>) -> Result<(), String> {
    match pat {
        Pattern::Wildcard => Ok(()),
        Pattern::Var(v) => { env.insert(v.clone(), scrutinee.clone()); Ok(()) }
        Pattern::Bool(_) => if scrutinee == &Type::Bool { Ok(()) } else { Err("bool pattern on non-bool match".to_string()) },
        Pattern::Number(_) => if scrutinee == &Type::Number { Ok(()) } else { Err("number pattern on non-number match".to_string()) },
    }
}

fn infer_call(name: &str, args: &[Expr], env: &HashMap<String, Type>, imports: &HashMap<String, String>) -> Result<Type, String> {
    let arg_types: Vec<Type> = args.iter().map(|a| infer_expr(a, env, imports)).collect::<Result<Vec<_>, _>>()?;
    let resolved_name = resolve_name(name, imports);

    if let Some(Type::Function { params, ret }) = env.get(&resolved_name).or_else(|| env.get(name)) {
        if params.len() != arg_types.len() { return Err(format!("function `{name}` expects {} args", params.len())); }
        return Ok(*ret.clone());
    }

    match resolved_name.as_str() {
        "std::print" | "print" => { if arg_types.len() != 1 { return Err("print expects 1 argument".to_string()); } Ok(arg_types[0].clone()) }
        "std::sqrt" | "sqrt" => expect_exact(name, &arg_types, &[Type::Number]).map(|_| Type::Number),
        "std::min" | "std::max" | "min" | "max" => expect_exact(name, &arg_types, &[Type::Number, Type::Number]).map(|_| Type::Number),
        "std::now_ms" | "now_ms" => expect_exact(name, &arg_types, &[]).map(|_| Type::Number),
        "std::vec" | "vec" => { for t in &arg_types { if *t != Type::Number { return Err("vec expects number arguments".to_string()); } } Ok(Type::VectorNumber) }
        "std::len" | "len" => expect_exact(name, &arg_types, &[Type::VectorNumber]).map(|_| Type::Number),
        "Some" => { if arg_types.len() != 1 { return Err("Some expects 1 argument".to_string()); } Ok(Type::Option(Box::new(arg_types[0].clone()))) }
        "None" => { if !arg_types.is_empty() { return Err("None expects 0 arguments".to_string()); } Ok(Type::Option(Box::new(Type::Unknown))) }
        "Ok" => { if arg_types.len() != 1 { return Err("Ok expects 1 argument".to_string()); } Ok(Type::Result(Box::new(arg_types[0].clone()), Box::new(Type::Unknown))) }
        "Err" => { if arg_types.len() != 1 { return Err("Err expects 1 argument".to_string()); } Ok(Type::Result(Box::new(Type::Unknown), Box::new(arg_types[0].clone()))) }
        _ => Err(format!("unknown function `{name}`")),
    }
}

fn type_compatible(a: &Type, b: &Type) -> bool { a == b || *a == Type::Unknown || *b == Type::Unknown }
fn merge_type(a: Type, b: Type) -> Type { if a == Type::Unknown { b } else { a } }
fn expect_exact(name: &str, got: &[Type], expected: &[Type]) -> Result<(), String> { if got == expected { Ok(()) } else { Err(format!("function `{name}` argument mismatch: expected {expected:?}, got {got:?}")) } }

pub fn resolve_namespaces(program: &[Stmt]) -> Vec<Stmt> {
    let mut imports: HashMap<String, String> = HashMap::new();
    for stmt in program {
        if let Stmt::Import { path, alias } = stmt {
            let key = alias.clone().unwrap_or_else(|| path.split("::").last().unwrap_or(path).to_string());
            imports.insert(key, path.clone());
        }
    }

    fn resolve_name(name: &str, imports: &HashMap<String, String>) -> String {
        if let Some((head, tail)) = name.split_once("::") {
            if let Some(path) = imports.get(head) {
                return format!("{}::{}", path, tail);
            }
        }
        name.to_string()
    }

    fn map_expr(expr: &Expr, imports: &HashMap<String, String>) -> Expr {
        match expr {
            Expr::Number(n) => Expr::Number(*n),
            Expr::Bool(b) => Expr::Bool(*b),
            Expr::String(s) => Expr::String(s.clone()),
            Expr::Var(v) => Expr::Var(resolve_name(v, imports)),
            Expr::Tuple(items) => Expr::Tuple(items.iter().map(|e| map_expr(e, imports)).collect()),
            Expr::Binary(l, op, r) => Expr::Binary(Box::new(map_expr(l, imports)), *op, Box::new(map_expr(r, imports))),
            Expr::Call { name, args } => Expr::Call { name: resolve_name(name, imports), args: args.iter().map(|e| map_expr(e, imports)).collect() },
            Expr::If { cond, then_branch, else_branch } => Expr::If { cond: Box::new(map_expr(cond, imports)), then_branch: Box::new(map_expr(then_branch, imports)), else_branch: Box::new(map_expr(else_branch, imports)) },
            Expr::Match { scrutinee, arms } => Expr::Match { scrutinee: Box::new(map_expr(scrutinee, imports)), arms: arms.iter().map(|(p,e)| (p.clone(), map_expr(e, imports))).collect() },
            Expr::Lambda { params, body } => Expr::Lambda { params: params.clone(), body: Box::new(map_expr(body, imports)) },
        }
    }

    program.iter().map(|s| match s {
        Stmt::Import { path, alias } => Stmt::Import { path: path.clone(), alias: alias.clone() },
        Stmt::Signal(n, e) => Stmt::Signal(n.clone(), map_expr(e, &imports)),
        Stmt::Reactive(n, e) => Stmt::Reactive(n.clone(), map_expr(e, &imports)),
        Stmt::Set(n, e) => Stmt::Set(resolve_name(n, &imports), map_expr(e, &imports)),
        Stmt::FnDef { name, params, body } => Stmt::FnDef { name: resolve_name(name, &imports), params: params.clone(), body: map_expr(body, &imports) },
        Stmt::Expr(e) => Stmt::Expr(map_expr(e, &imports)),
    }).collect()
}
