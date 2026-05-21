use std::collections::HashMap;

use crate::hir::{Block, Expr, Pattern, Stmt};

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

#[derive(Clone, Debug)]
struct EnvStack {
    scopes: Vec<HashMap<String, Type>>,
}

impl EnvStack {
    fn new() -> Self { Self { scopes: vec![HashMap::new()] } }
    fn push_scope(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop_scope(&mut self) { self.scopes.pop(); }

    fn declare(&mut self, name: String, ty: Type) -> Result<(), String> {
        let current = self.scopes.last_mut().expect("env always has one scope");
        if current.contains_key(&name) {
            return Err(format!("name `{name}` is already defined in this scope"));
        }
        current.insert(name, ty);
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    fn assign(&mut self, name: &str, ty: Type) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), ty);
                return Ok(());
            }
        }
        Err(format!("cannot set unknown name `{name}`"))
    }
}

pub fn type_check(program: &[Stmt]) -> Result<(), String> {
    let mut env = EnvStack::new();
    let mut imports: HashMap<String, String> = HashMap::new();

    for stmt in program {
        if let Stmt::Import { path, alias } = stmt {
            let key = alias.clone().unwrap_or_else(|| path.split("::").last().unwrap_or(path).to_string());
            imports.insert(key, path.clone());
        }
    }

    infer_stmts(program, &mut env, &imports)?;
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

fn infer_stmts(stmts: &[Stmt], env: &mut EnvStack, imports: &HashMap<String, String>) -> Result<Type, String> {
    let mut last = Type::Tuple(vec![]);
    for stmt in stmts {
        last = infer_stmt(stmt, env, imports)?;
    }
    Ok(last)
}

fn infer_stmt(stmt: &Stmt, env: &mut EnvStack, imports: &HashMap<String, String>) -> Result<Type, String> {
    match stmt {
        Stmt::Import { .. } => Ok(Type::Tuple(vec![])),
        Stmt::Signal(name, expr) | Stmt::Reactive(name, expr) => {
            let ty = infer_expr(expr, env, imports)?;
            env.declare(name.clone(), ty.clone())?;
            Ok(ty)
        }
        Stmt::Set(name, expr) => {
            let resolved = resolve_name(name, imports);
            let current = env.lookup(&resolved).or_else(|| env.lookup(name)).ok_or_else(|| format!("cannot set unknown name `{name}`"))?;
            let next = infer_expr(expr, env, imports)?;
            if !type_compatible(&current, &next) {
                return Err(format!("type mismatch in set `{name}`: expected {current:?}, got {next:?}"));
            }
            if env.lookup(&resolved).is_some() { env.assign(&resolved, next.clone())?; }
            else { env.assign(name, next.clone())?; }
            Ok(next)
        }
        Stmt::FnDef { name, params, body } => {
            let ret = infer_function_block(body, params, env, imports)?;
            let fty = Type::Function { params: vec![Type::Unknown; params.len()], ret: Box::new(ret) };
            env.declare(name.clone(), fty.clone())?;
            Ok(fty)
        }
        Stmt::Expr(expr) => infer_expr(expr, env, imports),
    }
}

fn infer_function_block(block: &Block, params: &[String], env: &EnvStack, imports: &HashMap<String, String>) -> Result<Type, String> {
    let mut local = env.clone();
    local.push_scope();
    for p in params {
        local.declare(p.clone(), Type::Unknown)?;
    }
    infer_block_expr(block, &mut local, imports)
}

fn infer_expr(expr: &Expr, env: &mut EnvStack, imports: &HashMap<String, String>) -> Result<Type, String> {
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
            env.lookup(&resolved).or_else(|| env.lookup(name)).ok_or_else(|| format!("unknown name `{name}`"))
        }
        Expr::Binary(l, _op, r) => {
            let lt = infer_expr(l, env, imports)?;
            let rt = infer_expr(r, env, imports)?;
            match (lt, rt) {
                (Type::Number, Type::Number) => Ok(Type::Number),
                (Type::Unknown, Type::Number) | (Type::Number, Type::Unknown) | (Type::Unknown, Type::Unknown) => Ok(Type::Number),
                (a, b) => Err(format!("binary op expects numbers, got {a:?} and {b:?}")),
            }
        }
        Expr::If { cond, then_branch, else_branch } => {
            let ct = infer_expr(cond, env, imports)?;
            if ct != Type::Bool { return Err("if condition must be bool".to_string()); }
            let tt = infer_block_expr(then_branch, env, imports)?;
            let et = infer_block_expr(else_branch, env, imports)?;
            if !type_compatible(&tt, &et) { return Err(format!("if branches type mismatch: {tt:?} vs {et:?}")); }
            Ok(merge_type(tt, et))
        }
        Expr::Match { scrutinee, arms } => {
            let st = infer_expr(scrutinee, env, imports)?;
            let mut out: Option<Type> = None;
            for (pat, expr) in arms {
                let mut arm_env = env.clone();
                arm_env.push_scope();
                bind_pattern(pat, &st, &mut arm_env)?;
                let ty = infer_expr(expr, &mut arm_env, imports)?;
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
            local.push_scope();
            for p in params { local.declare(p.clone(), Type::Unknown)?; }
            let ret = infer_expr(body, &mut local, imports)?;
            Ok(Type::Function { params: vec![Type::Unknown; params.len()], ret: Box::new(ret) })
        }
        Expr::Call { name, args } => infer_call(name, args, env, imports),
    }
}

fn infer_block_expr(block: &Block, env: &mut EnvStack, imports: &HashMap<String, String>) -> Result<Type, String> {
    env.push_scope();
    let result = infer_stmts(&block.stmts, env, imports);
    env.pop_scope();
    result
}

fn bind_pattern(pat: &Pattern, scrutinee: &Type, env: &mut EnvStack) -> Result<(), String> {
    match pat {
        Pattern::Wildcard => Ok(()),
        Pattern::Var(v) => env.declare(v.clone(), scrutinee.clone()),
        Pattern::Bool(_) => if scrutinee == &Type::Bool { Ok(()) } else { Err("bool pattern on non-bool match".to_string()) },
        Pattern::Number(_) => if scrutinee == &Type::Number { Ok(()) } else { Err("number pattern on non-number match".to_string()) },
    }
}

fn infer_call(name: &str, args: &[Expr], env: &mut EnvStack, imports: &HashMap<String, String>) -> Result<Type, String> {
    let arg_types: Vec<Type> = args.iter().map(|a| infer_expr(a, env, imports)).collect::<Result<Vec<_>, _>>()?;
    let resolved_name = resolve_name(name, imports);

    if let Some(Type::Function { params, ret }) = env.lookup(&resolved_name).or_else(|| env.lookup(name)) {
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


    fn map_block(block: &Block, imports: &HashMap<String, String>) -> Block {
        Block {
            stmts: block.stmts.iter().map(|s| match s {
                Stmt::Import { path, alias } => Stmt::Import { path: path.clone(), alias: alias.clone() },
                Stmt::Signal(n, e) => Stmt::Signal(n.clone(), map_expr(e, imports)),
                Stmt::Reactive(n, e) => Stmt::Reactive(n.clone(), map_expr(e, imports)),
                Stmt::Set(n, e) => Stmt::Set(resolve_name(n, imports), map_expr(e, imports)),
                Stmt::FnDef { name, params, body } => Stmt::FnDef { name: resolve_name(name, imports), params: params.clone(), body: map_block(body, imports) },
                Stmt::Expr(e) => Stmt::Expr(map_expr(e, imports)),
            }).collect()
        }
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
            Expr::If { cond, then_branch, else_branch } => Expr::If { cond: Box::new(map_expr(cond, imports)), then_branch: Box::new(map_block(then_branch, imports)), else_branch: Box::new(map_block(else_branch, imports)) },
            Expr::Match { scrutinee, arms } => Expr::Match { scrutinee: Box::new(map_expr(scrutinee, imports)), arms: arms.iter().map(|(p,e)| (p.clone(), map_expr(e, imports))).collect() },
            Expr::Lambda { params, body } => Expr::Lambda { params: params.clone(), body: Box::new(map_expr(body, imports)) },
        }
    }

    program.iter().map(|s| match s {
        Stmt::Import { path, alias } => Stmt::Import { path: path.clone(), alias: alias.clone() },
        Stmt::Signal(n, e) => Stmt::Signal(n.clone(), map_expr(e, &imports)),
        Stmt::Reactive(n, e) => Stmt::Reactive(n.clone(), map_expr(e, &imports)),
        Stmt::Set(n, e) => Stmt::Set(resolve_name(n, &imports), map_expr(e, &imports)),
        Stmt::FnDef { name, params, body } => Stmt::FnDef { name: resolve_name(name, &imports), params: params.clone(), body: map_block(body, &imports) },
        Stmt::Expr(e) => Stmt::Expr(map_expr(e, &imports)),
    }).collect()
}
