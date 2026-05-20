use crate::hir::{Expr, Op, Stmt};

pub fn parse_program(src: &str) -> Result<Vec<Stmt>, String> {
    let mut out = Vec::new();
    for (idx, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let stmt = if let Some(rest) = line.strip_prefix("signal ") {
            let (name, expr) = split_assignment(rest, idx + 1)?;
            Stmt::Signal(name.to_string(), parse_expr(expr)?)
        } else if let Some(rest) = line.strip_prefix("reactive ") {
            let (name, expr) = split_assignment(rest, idx + 1)?;
            Stmt::Reactive(name.to_string(), parse_expr(expr)?)
        } else if let Some(rest) = line.strip_prefix("set ") {
            let (name, expr) = split_assignment(rest, idx + 1)?;
            Stmt::Set(name.to_string(), parse_expr(expr)?)
        } else {
            Stmt::Expr(parse_expr(line)?)
        };
        out.push(stmt);
    }
    Ok(out)
}

fn split_assignment(input: &str, line_no: usize) -> Result<(&str, &str), String> {
    let mut parts = input.splitn(2, '=');
    let left = parts.next().ok_or_else(|| format!("line {line_no}: expected assignment"))?.trim();
    let right = parts.next().ok_or_else(|| format!("line {line_no}: expected '='"))?.trim();
    if left.is_empty() || right.is_empty() { return Err(format!("line {line_no}: malformed assignment")); }
    Ok((left, right))
}

fn parse_expr(input: &str) -> Result<Expr, String> {
    let tokens = tokenize(input)?;
    let (expr, pos) = parse_expr_bp(&tokens, 0, 0)?;
    if pos != tokens.len() { return Err(format!("unexpected token `{}`", tokens[pos])); }
    Ok(expr)
}

fn parse_expr_bp(tokens: &[String], mut pos: usize, min_bp: u8) -> Result<(Expr, usize), String> {
    let mut lhs = if pos >= tokens.len() { return Err("unexpected end of expression".to_string()); }
    else if tokens[pos] == "(" {
        pos += 1;
        let (expr, next) = parse_expr_bp(tokens, pos, 0)?;
        pos = next;
        if tokens.get(pos).map(|s| s.as_str()) != Some(")") { return Err("missing ')'".to_string()); }
        pos += 1;
        expr
    } else if let Ok(n) = tokens[pos].parse::<f64>() {
        pos += 1;
        Expr::Number(n)
    } else {
        let ident = tokens[pos].clone();
        pos += 1;
        if tokens.get(pos).map(|s| s.as_str()) == Some("(") {
            pos += 1;
            let mut args = Vec::new();
            if tokens.get(pos).map(|s| s.as_str()) != Some(")") {
                loop {
                    let (arg, next) = parse_expr_bp(tokens, pos, 0)?;
                    args.push(arg);
                    pos = next;
                    match tokens.get(pos).map(|s| s.as_str()) {
                        Some(",") => pos += 1,
                        Some(")") => break,
                        _ => return Err("expected ',' or ')' in call".to_string()),
                    }
                }
            }
            if tokens.get(pos).map(|s| s.as_str()) != Some(")") { return Err("missing ')' after call".to_string()); }
            pos += 1;
            Expr::Call { name: ident, args }
        } else {
            Expr::Var(ident)
        }
    };

    while pos < tokens.len() {
        let op = match tokens[pos].as_str() {
            "+" => Op::Add, "-" => Op::Sub, "*" => Op::Mul, "/" => Op::Div, ")" | "," => break,
            _ => return Err(format!("unknown operator `{}`", tokens[pos])),
        };
        let (l, r) = binding(op);
        if l < min_bp { break; }
        pos += 1;
        let (rhs, next) = parse_expr_bp(tokens, pos, r)?;
        pos = next;
        lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
    }
    Ok((lhs, pos))
}

fn binding(op: Op) -> (u8, u8) { match op { Op::Add | Op::Sub => (1,2), Op::Mul | Op::Div => (3,4) } }

fn tokenize(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut buf = String::new();
    for ch in input.chars() {
        match ch {
            ' ' | '\t' => { if !buf.is_empty() { tokens.push(std::mem::take(&mut buf)); } }
            '(' | ')' | '+' | '-' | '*' | '/' | ',' => {
                if !buf.is_empty() { tokens.push(std::mem::take(&mut buf)); }
                tokens.push(ch.to_string());
            }
            c if c.is_ascii_alphanumeric() || c == '_' || c == '.' => buf.push(c),
            _ => return Err(format!("invalid character `{ch}`")),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }
    Ok(tokens)
}
