use crate::hir::{Expr, Op, Stmt};

pub fn emit_text(stmts: &[Stmt]) -> String {
    let mut out = String::new();
    for stmt in stmts {
        match stmt {
            Stmt::Signal(name, expr) => out.push_str(&format!("signal {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Reactive(name, expr) => out.push_str(&format!("reactive {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Set(name, expr) => out.push_str(&format!("set {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Expr(expr) => out.push_str(&format!("expr {}\n", expr_to_rpn(expr))),
        }
    }
    out
}

pub fn emit_binary(stmts: &[Stmt]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"FLXB");
    out.push(2); // bytecode v2
    out.extend_from_slice(&(stmts.len() as u32).to_le_bytes());

    for stmt in stmts {
        let (kind, name, payload) = match stmt {
            Stmt::Signal(name, expr) => (0u8, name.as_str(), encode_expr(expr)),
            Stmt::Reactive(name, expr) => (1u8, name.as_str(), encode_expr(expr)),
            Stmt::Set(name, expr) => (2u8, name.as_str(), encode_expr(expr)),
            Stmt::Expr(expr) => (3u8, "", encode_expr(expr)),
        };
        out.push(kind);
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
    }
    out
}

fn encode_expr(expr: &Expr) -> Vec<u8> {
    let mut out = Vec::new();
    encode_expr_into(expr, &mut out);
    out
}

fn encode_expr_into(expr: &Expr, out: &mut Vec<u8>) {
    match expr {
        Expr::Number(n) => {
            out.push(1);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Expr::Bool(b) => {
            out.push(2);
            out.push(if *b { 1 } else { 0 });
        }
        Expr::Var(v) => {
            out.push(3);
            out.extend_from_slice(&(v.len() as u16).to_le_bytes());
            out.extend_from_slice(v.as_bytes());
        }
        Expr::Binary(l, op, r) => {
            encode_expr_into(l, out);
            encode_expr_into(r, out);
            out.push(match op { Op::Add => 4, Op::Sub => 5, Op::Mul => 6, Op::Div => 7 });
        }
        Expr::Call { name, args } => {
            for a in args { encode_expr_into(a, out); }
            out.push(8);
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.push(args.len() as u8);
        }
    }
}

fn expr_to_rpn(expr: &Expr) -> String { match expr { Expr::Number(n)=>n.to_string(), Expr::Bool(b)=>b.to_string(), Expr::Var(v)=>v.clone(), Expr::Binary(l,op,r)=>format!("{} {} {}",expr_to_rpn(l),expr_to_rpn(r),op_str(*op)), Expr::Call{name,args}=>{let mut p:Vec<String>=args.iter().map(expr_to_rpn).collect(); p.push(format!("@call:{}:{}",name,args.len())); p.join(" ")}} }
fn op_str(op: Op) -> &'static str { match op { Op::Add=>"+", Op::Sub=>"-", Op::Mul=>"*", Op::Div=>"/" } }
