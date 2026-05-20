use crate::hir::{Expr, Op, Pattern, Stmt};

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
    out.push(3); // bytecode v3
    let emitted_count = stmts.iter().filter(|s| !matches!(s, Stmt::Import { .. })).count() as u32;
    out.extend_from_slice(&emitted_count.to_le_bytes());

    for stmt in stmts {
        let (kind, name, payload) = match stmt {
            Stmt::Import { .. } => continue,
            Stmt::Signal(name, expr) => (0u8, name.as_str(), encode_expr(expr)),
            Stmt::Reactive(name, expr) => (1u8, name.as_str(), encode_expr(expr)),
            Stmt::Set(name, expr) => (2u8, name.as_str(), encode_expr(expr)),
            Stmt::Expr(expr) => (3u8, "", encode_expr(expr)),
            Stmt::FnDef { name, .. } => panic!("emit_binary fail-fast: fn `{name}` is unsupported in runtime bytecode path"),
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
        Expr::String(_) => panic!("emit_binary fail-fast: string is unsupported in runtime bytecode path"),
        Expr::Tuple(_) => panic!("emit_binary fail-fast: tuple is unsupported in runtime bytecode path"),
        Expr::If { cond, then_branch, else_branch } => {
            encode_expr_into(cond, out);
            out.push(12); // branch_if_false
            let branch_pos = out.len();
            out.extend_from_slice(&0u32.to_le_bytes());
            encode_expr_into(then_branch, out);
            out.push(11); // jump
            let jump_pos = out.len();
            out.extend_from_slice(&0u32.to_le_bytes());
            let else_start = out.len() as u32;
            out[branch_pos..branch_pos + 4].copy_from_slice(&else_start.to_le_bytes());
            encode_expr_into(else_branch, out);
            let end = out.len() as u32;
            out[jump_pos..jump_pos + 4].copy_from_slice(&end.to_le_bytes());
        }
        Expr::Match { scrutinee, arms } => {
            if arms.is_empty() { panic!("emit_binary fail-fast: match must have at least one arm"); }
            encode_expr_into(scrutinee, out);
            let mut jumps_to_end = Vec::new();
            for (idx, (pat, expr)) in arms.iter().enumerate() {
                let is_last = idx + 1 == arms.len();
                match pat {
                    Pattern::Wildcard | Pattern::Var(_) if is_last => {
                        out.push(14); // pop scrutinee
                        encode_expr_into(expr, out);
                    }
                    Pattern::Wildcard | Pattern::Var(_) => {
                        out.push(14);
                        encode_expr_into(expr, out);
                        out.push(11);
                        let jp = out.len(); out.extend_from_slice(&0u32.to_le_bytes()); jumps_to_end.push(jp);
                    }
                    Pattern::Bool(b) => {
                        out.push(13);
                        out.push(2); out.push(if *b {1} else {0});
                        out.push(15);
                        out.push(12);
                        let branch_pos = out.len(); out.extend_from_slice(&0u32.to_le_bytes());
                        out.push(14);
                        encode_expr_into(expr, out);
                        out.push(11);
                        let jp = out.len(); out.extend_from_slice(&0u32.to_le_bytes()); jumps_to_end.push(jp);
                        let next = out.len() as u32;
                        out[branch_pos..branch_pos+4].copy_from_slice(&next.to_le_bytes());
                    }
                    Pattern::Number(n) => {
                        out.push(13);
                        out.push(1); out.extend_from_slice(&n.to_le_bytes());
                        out.push(16);
                        out.push(12);
                        let branch_pos = out.len(); out.extend_from_slice(&0u32.to_le_bytes());
                        out.push(14);
                        encode_expr_into(expr, out);
                        out.push(11);
                        let jp = out.len(); out.extend_from_slice(&0u32.to_le_bytes()); jumps_to_end.push(jp);
                        let next = out.len() as u32;
                        out[branch_pos..branch_pos+4].copy_from_slice(&next.to_le_bytes());
                    }
                }
            }
            let end = out.len() as u32;
            for jp in jumps_to_end { out[jp..jp+4].copy_from_slice(&end.to_le_bytes()); }
        }
        Expr::Lambda { .. } => {
            panic!("emit_binary fail-fast: lambda is unsupported in runtime bytecode path");
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
