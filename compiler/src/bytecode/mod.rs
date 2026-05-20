use crate::hir::{Expr, Op, Stmt};

pub fn emit_text(stmts: &[Stmt]) -> String {
    let mut out = String::new();
    for stmt in stmts {
        match stmt {
            Stmt::Import { path, alias } => {
                if let Some(a) = alias { out.push_str(&format!("import {} as {}\n", path, a)); }
                else { out.push_str(&format!("import {}\n", path)); }
            }
            Stmt::Signal(name, expr) => out.push_str(&format!("signal {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Reactive(name, expr) => out.push_str(&format!("reactive {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Set(name, expr) => out.push_str(&format!("set {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Expr(expr) => out.push_str(&format!("expr {}\n", expr_to_rpn(expr))),
            Stmt::FnDef { name, params, body } => out.push_str(&format!("fn {}({}) {{ {} }}\n", name, params.join(", "), expr_to_rpn(body))),
        }
    }
    out
}

pub fn emit_binary(stmts: &[Stmt]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"FLXB");
    out.push(2); // bytecode v2
    let emitted_count = stmts.iter().filter(|s| !matches!(s, Stmt::Import { .. })).count() as u32;
    out.extend_from_slice(&emitted_count.to_le_bytes());

    for stmt in stmts {
        let (kind, name, payload) = match stmt {
            Stmt::Import { .. } => continue,
            Stmt::Signal(name, expr) => (0u8, name.as_str(), encode_expr(expr)),
            Stmt::Reactive(name, expr) => (1u8, name.as_str(), encode_expr(expr)),
            Stmt::Set(name, expr) => (2u8, name.as_str(), encode_expr(expr)),
            Stmt::Expr(expr) => (3u8, "", encode_expr(expr)),
            Stmt::FnDef { .. } => panic!("binary emission for fn is not implemented yet"),
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
        Expr::String(s) => {
            out.push(9);
            out.extend_from_slice(&(s.len() as u16).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        Expr::Tuple(items) => {
            for it in items { encode_expr_into(it, out); }
            out.push(10);
            out.push(items.len() as u8);
        }
        Expr::If { cond, then_branch, else_branch } => {
            encode_expr_into(cond, out);
            encode_expr_into(then_branch, out);
            encode_expr_into(else_branch, out);
            // temporary lowering: call builtin if3(cond, then, else)
            out.push(8);
            out.extend_from_slice(&(3u16).to_le_bytes());
            out.extend_from_slice(b"if3");
            out.push(3);
        }
        Expr::Match { .. } => {
            panic!("bytecode emission for `match` is not implemented yet");
        }
        Expr::Lambda { .. } => {
            panic!("bytecode emission for lambda is not implemented yet");
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

fn expr_to_rpn(expr: &Expr) -> String { match expr { Expr::Number(n)=>n.to_string(), Expr::Bool(b)=>b.to_string(), Expr::String(s)=>format!("\"{}\"",s), Expr::Var(v)=>v.clone(), Expr::Tuple(items)=>format!("({})", items.iter().map(expr_to_rpn).collect::<Vec<_>>().join(", ")), Expr::Binary(l,op,r)=>format!("{} {} {}",expr_to_rpn(l),expr_to_rpn(r),op_str(*op)), Expr::If{cond,then_branch,else_branch}=>format!("if {} {{ {} }} else {{ {} }}",expr_to_rpn(cond),expr_to_rpn(then_branch),expr_to_rpn(else_branch)), Expr::Match{..}=>"<match>".to_string(), Expr::Lambda{..}=>"<lambda>".to_string(), Expr::Call{name,args}=>{let mut p:Vec<String>=args.iter().map(expr_to_rpn).collect(); p.push(format!("@call:{}:{}",name,args.len())); p.join(" ")}} }
fn op_str(op: Op) -> &'static str { match op { Op::Add=>"+", Op::Sub=>"-", Op::Mul=>"*", Op::Div=>"/" } }
