use crate::hir::Stmt;

pub fn emit_text(stmts: &[Stmt]) -> String {
    let mut out = String::new();
    for stmt in stmts {
        match stmt {
            Stmt::Signal(name, expr) => out.push_str(&format!("signal {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Reactive(name, expr) => out.push_str(&format!("reactive {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Set(name, expr) => out.push_str(&format!("set {} = {}\n", name, expr_to_rpn(expr))),
            Stmt::Print(name) => out.push_str(&format!("print {}\n", name)),
        }
    }
    out
}

fn expr_to_rpn(expr: &crate::hir::Expr) -> String {
    match expr {
        crate::hir::Expr::Number(n) => n.to_string(),
        crate::hir::Expr::Var(v) => v.clone(),
        crate::hir::Expr::Binary(l, op, r) => format!("{} {} {}", expr_to_rpn(l), expr_to_rpn(r), op_str(*op)),
    }
}

fn op_str(op: crate::hir::Op) -> &'static str { match op { crate::hir::Op::Add=>"+", crate::hir::Op::Sub=>"-", crate::hir::Op::Mul=>"*", crate::hir::Op::Div=>"/" } }
