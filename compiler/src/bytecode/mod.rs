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

fn expr_to_rpn(expr: &Expr) -> String {
    match expr {
        Expr::Number(n) => n.to_string(),
        Expr::Var(v) => v.clone(),
        Expr::Binary(l, op, r) => format!("{} {} {}", expr_to_rpn(l), expr_to_rpn(r), op_str(*op)),
        Expr::Call { name, args } => {
            let mut parts: Vec<String> = args.iter().map(expr_to_rpn).collect();
            parts.push(format!("@call:{}:{}", name, args.len()));
            parts.join(" ")
        }
    }
}

fn op_str(op: Op) -> &'static str { match op { Op::Add=>"+", Op::Sub=>"-", Op::Mul=>"*", Op::Div=>"/" } }
