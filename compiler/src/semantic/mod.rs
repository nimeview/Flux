use std::collections::HashMap;

use crate::hir::{Expr, LangTypeRef, Pattern, Stmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeMode {
    Strict,
    ExperimentalInfer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LangType {
    Number,
    Bool,
    String,
    Tuple(Vec<LangType>),
    VectorNumber,
    Option(Box<LangType>),
    Result(Box<LangType>, Box<LangType>),
    Function { params: Vec<LangType>, ret: Box<LangType> },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueType {
    Number,
    Bool,
    String,
    Tuple,
    Vector,
    Option,
    Result,
    Function,
}

#[derive(Clone, Debug)]
struct TcError {
    msg: String,
}

impl TcError {
    fn new(kind: &str, expected: &str, got: &str, context: &str) -> Self {
        Self { msg: format!("<memory>:1:1: {kind}: expected {expected}, got {got}; context: {context}") }
    }
}

pub fn type_check(program: &[Stmt]) -> Result<(), String> { type_check_with_mode(program, TypeMode::Strict) }

pub fn type_check_with_mode(program: &[Stmt], mode: TypeMode) -> Result<(), String> {
    let mut env: HashMap<String, LangType> = HashMap::new();
    let mut imports: HashMap<String, String> = HashMap::new();

    for stmt in program {
        if let Stmt::Import { path, alias } = stmt {
            let key = alias.clone().unwrap_or_else(|| path.split("::").last().unwrap_or(path).to_string());
            imports.insert(key, path.clone());
        }
    }

    for stmt in program {
        match stmt {
            Stmt::Import { .. } | Stmt::Package { .. } => {}
            Stmt::Signal { name, expr, .. } | Stmt::Reactive { name, expr, .. } => {
                let ty = infer_expr(expr, &env, &imports, mode)?;
                env.insert(name.clone(), ty);
            }
            Stmt::Set(name, expr) => {
                let resolved = resolve_name(name, &imports);
                let current = env.get(&resolved).or_else(|| env.get(name)).ok_or_else(|| format!("<memory>:1:1: cannot set unknown name `{name}`"))?.clone();
                let next = infer_expr(expr, &env, &imports, mode)?;
                if !type_compatible(&current, &next, mode) {
                    return Err(TcError::new("set type mismatch", &format!("{current:?}"), &format!("{next:?}"), &format!("set {name}")).msg);
                }
            }
            Stmt::FnDef { name, params, body, param_types, .. } => {
                let mut local = env.clone();
                let mut sig_params = Vec::with_capacity(params.len());
                for (idx, p) in params.iter().enumerate() {
                    let annotated = match param_types.get(idx).and_then(|x| x.as_ref()) { Some(LangTypeRef::Number) => LangType::Number, Some(LangTypeRef::Bool) => LangType::Bool, Some(LangTypeRef::String) => LangType::String, None => LangType::Unknown };
                    local.insert(p.clone(), annotated.clone());
                    sig_params.push(annotated);
                }
                let ret = infer_expr(body, &local, &imports, mode)?;
                env.insert(name.clone(), LangType::Function { params: sig_params, ret: Box::new(ret) });
            }
            Stmt::Expr(expr) => { let _ = infer_expr(expr, &env, &imports, mode)?; }
        }
    }

    Ok(())
}

fn resolve_name(name: &str, imports: &HashMap<String, String>) -> String { if let Some((head, tail)) = name.split_once("::") { if let Some(path) = imports.get(head) { return format!("{}::{}", path, tail); } } name.to_string() }

fn infer_expr(expr: &Expr, env: &HashMap<String, LangType>, imports: &HashMap<String, String>, mode: TypeMode) -> Result<LangType, String> {
    match expr {
        Expr::Number(_) => Ok(LangType::Number),
        Expr::Bool(_) => Ok(LangType::Bool),
        Expr::String(_) => Ok(LangType::String),
        Expr::Tuple(items) => Ok(LangType::Tuple(items.iter().map(|i| infer_expr(i, env, imports, mode)).collect::<Result<Vec<_>, _>>()?)),
        Expr::Var(name) => {
            let resolved = resolve_name(name, imports);
            env.get(&resolved).or_else(|| env.get(name)).cloned().ok_or_else(|| format!("<memory>:1:1: unknown name `{name}`"))
        }
        Expr::Binary(l, _op, r) => {
            let lt = infer_expr(l, env, imports, mode)?;
            let rt = infer_expr(r, env, imports, mode)?;
            if lt == LangType::Number && rt == LangType::Number { return Ok(LangType::Number); }
            if mode == TypeMode::ExperimentalInfer && (lt == LangType::Unknown || rt == LangType::Unknown) { return Ok(LangType::Number); }
            Err(TcError::new("binary op type mismatch", "Number and Number", &format!("{lt:?} and {rt:?}"), "binary expression").msg)
        }
        Expr::If { cond, then_branch, else_branch } => {
            let ct = infer_expr(cond, env, imports, mode)?;
            if ct != LangType::Bool { return Err(TcError::new("if condition", "Bool", &format!("{ct:?}"), "if condition").msg); }
            let tt = infer_expr(then_branch, env, imports, mode)?;
            let et = infer_expr(else_branch, env, imports, mode)?;
            if !type_compatible(&tt, &et, mode) { return Err(TcError::new("if branches type mismatch", &format!("{tt:?}"), &format!("{et:?}"), "if expression").msg); }
            Ok(merge_type(tt, et))
        }
        Expr::Match { scrutinee, arms } => {
            let st = infer_expr(scrutinee, env, imports, mode)?;
            let mut out: Option<LangType> = None;
            for (pat, expr) in arms {
                let mut local = env.clone();
                bind_pattern(pat, &st, &mut local)?;
                let ty = infer_expr(expr, &local, imports, mode)?;
                out = Some(match out { None => ty, Some(prev) => { if !type_compatible(&prev, &ty, mode) { return Err(TcError::new("match arm type mismatch", &format!("{prev:?}"), &format!("{ty:?}"), "match expression").msg); } merge_type(prev, ty) } });
            }
            out.ok_or_else(|| "<memory>:1:1: match must have at least one arm".to_string())
        }
        Expr::Lambda { params, body } => {
            let mut local = env.clone();
            for p in params { local.insert(p.clone(), LangType::Unknown); }
            let ret = infer_expr(body, &local, imports, mode)?;
            Ok(LangType::Function { params: vec![LangType::Unknown; params.len()], ret: Box::new(ret) })
        }
        Expr::Call { name, args } => infer_call(name, args, env, imports, mode),
    }
}

fn bind_pattern(pat: &Pattern, scrutinee: &LangType, env: &mut HashMap<String, LangType>) -> Result<(), String> { match pat { Pattern::Wildcard => Ok(()), Pattern::Var(v) => { env.insert(v.clone(), scrutinee.clone()); Ok(()) }, Pattern::Bool(_) => if scrutinee == &LangType::Bool { Ok(()) } else { Err("<memory>:1:1: bool pattern on non-bool match".to_string()) }, Pattern::Number(_) => if scrutinee == &LangType::Number { Ok(()) } else { Err("<memory>:1:1: number pattern on non-number match".to_string()) }, } }

fn infer_call(name: &str, args: &[Expr], env: &HashMap<String, LangType>, imports: &HashMap<String, String>, mode: TypeMode) -> Result<LangType, String> {
    let arg_types: Vec<LangType> = args.iter().map(|a| infer_expr(a, env, imports, mode)).collect::<Result<Vec<_>, _>>()?;
    let resolved_name = resolve_name(name, imports);
    if let Some(LangType::Function { params, ret }) = env.get(&resolved_name).or_else(|| env.get(name)) {
        if params.len() != arg_types.len() { return Err(format!("<memory>:1:1: function `{name}` arity mismatch: expected {}, got {}", params.len(), arg_types.len())); }
        for (expected, got) in params.iter().zip(arg_types.iter()) {
            if !type_compatible(expected, got, mode) { return Err(TcError::new("call arg type mismatch", &format!("{expected:?}"), &format!("{got:?}"), &format!("call {name}")).msg); }
        }
        return Ok(*ret.clone());
    }
    match resolved_name.as_str() {
        "std::sqrt" | "sqrt" => expect_exact(name, &arg_types, &[LangType::Number], mode).map(|_| LangType::Number),
        "std::min" | "std::max" | "min" | "max" => expect_exact(name, &arg_types, &[LangType::Number, LangType::Number], mode).map(|_| LangType::Number),
        "std::now_ms" | "now_ms" => expect_exact(name, &arg_types, &[], mode).map(|_| LangType::Number),
        "std::print" | "print" => { if arg_types.len() != 1 { Err("<memory>:1:1: print arity mismatch: expected 1".to_string()) } else { Ok(arg_types[0].clone()) } },
        "std::vec" | "vec" => {
            for g in &arg_types {
                if !type_compatible(&LangType::Number, g, mode) {
                    return Err(TcError::new("call arg type mismatch", "Number", &format!("{g:?}"), &format!("call {name}")).msg);
                }
            }
            Ok(LangType::VectorNumber)
        }
        "std::len" | "len" => expect_exact(name, &arg_types, &[LangType::VectorNumber], mode).map(|_| LangType::Number),
        _ => Err(format!("<memory>:1:1: unknown function `{name}`")),
    }
}

fn type_compatible(a: &LangType, b: &LangType, mode: TypeMode) -> bool { a == b || (mode == TypeMode::ExperimentalInfer && (*a == LangType::Unknown || *b == LangType::Unknown)) }
fn merge_type(a: LangType, b: LangType) -> LangType { if a == LangType::Unknown { b } else { a } }
fn expect_exact(name: &str, got: &[LangType], expected: &[LangType], mode: TypeMode) -> Result<(), String> { if got.len() != expected.len() { return Err(format!("<memory>:1:1: function `{name}` arity mismatch: expected {}, got {}", expected.len(), got.len())); } for (e,g) in expected.iter().zip(got.iter()) { if !type_compatible(e,g,mode) { return Err(TcError::new("call arg type mismatch", &format!("{e:?}"), &format!("{g:?}"), &format!("call {name}")).msg); } } Ok(()) }

pub fn resolve_namespaces(program: &[Stmt]) -> Vec<Stmt> { program.to_vec() }
