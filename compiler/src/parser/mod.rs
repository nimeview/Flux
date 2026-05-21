use crate::hir::{Block, Expr, Op, Pattern, Stmt};

pub fn parse_program(src: &str) -> Result<Vec<Stmt>, String> {
    let logical_lines = split_logical_statements(src)?;
    parse_statement_list(&logical_lines)
}

fn parse_statement_list(logical_lines: &[(usize, String)]) -> Result<Vec<Stmt>, String> {
    let mut out = Vec::new();
    for (line_no, line) in logical_lines {
        let stmt = if let Some(rest) = line.strip_prefix("import ") {
            parse_import_stmt(rest, *line_no)?
        } else if let Some(rest) = line.strip_prefix("signal ") {
            let (name, expr) = split_assignment(rest, *line_no)?;
            Stmt::Signal(name.to_string(), parse_expr(expr, *line_no)?)
        } else if let Some(rest) = line.strip_prefix("reactive ") {
            let (name, expr) = split_assignment(rest, *line_no)?;
            Stmt::Reactive(name.to_string(), parse_expr(expr, *line_no)?)
        } else if let Some(rest) = line.strip_prefix("set ") {
            let (name, expr) = split_assignment(rest, *line_no)?;
            Stmt::Set(name.to_string(), parse_expr(expr, *line_no)?)
        } else if let Some(rest) = line.strip_prefix("fn ") {
            parse_fn_stmt(rest, *line_no)?
        } else {
            Stmt::Expr(parse_expr(line, *line_no)?)
        };
        out.push(stmt);
    }
    Ok(out)
}

fn split_logical_statements(src: &str) -> Result<Vec<(usize, String)>, String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut start_line = 1usize;
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let mut in_string = false;

    for (idx, raw) in src.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }

        if buf.is_empty() { start_line = line_no; } else { buf.push(' '); }
        buf.push_str(trimmed);

        for ch in trimmed.chars() {
            if ch == '"' { in_string = !in_string; }
            else if !in_string {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    '(' => paren_depth += 1,
                    ')' => paren_depth -= 1,
                    _ => {}
                }
            }
        }

        if brace_depth < 0 || paren_depth < 0 { return Err(format!("line {line_no}, col 1: unmatched closing delimiter")); }
        if brace_depth == 0 && paren_depth == 0 && !in_string {
            out.push((start_line, buf.trim().to_string()));
            buf.clear();
        }
    }

    if in_string { return Err("unterminated string literal at end of file".to_string()); }
    if brace_depth != 0 || paren_depth != 0 { return Err("unterminated block or expression at end of file".to_string()); }
    Ok(out)
}

fn parse_import_stmt(input: &str, line_no: usize) -> Result<Stmt, String> {
    let input = input.trim();
    if input.is_empty() { return Err(format!("line {line_no}, col 1: expected import path")); }
    if let Some((path, alias)) = input.split_once(" as ") {
        return Ok(Stmt::Import { path: path.trim().to_string(), alias: Some(alias.trim().to_string()) });
    }
    Ok(Stmt::Import { path: input.to_string(), alias: None })
}

fn parse_fn_stmt(input: &str, line_no: usize) -> Result<Stmt, String> {
    let open = input.find('(').ok_or_else(|| format!("line {line_no}, col 1: expected '(' in fn"))?;
    let close = input.rfind(')').ok_or_else(|| format!("line {line_no}, col 1: expected ')' in fn"))?;
    let (body_src, _) = extract_braced(input, line_no)?;
    let name = input[..open].trim().to_string();
    let params_raw = &input[open + 1..close];
    let params = if params_raw.trim().is_empty() { vec![] } else { params_raw.split(',').map(|p| p.trim().to_string()).collect() };
    let body_lines = split_logical_statements(&body_src)?;
    let body = Block { stmts: parse_statement_list(&body_lines)? };
    Ok(Stmt::FnDef { name, params, body })
}

fn split_assignment(input: &str, line_no: usize) -> Result<(&str, &str), String> {
    let mut parts = input.splitn(2, '=');
    let left = parts.next().ok_or_else(|| format!("line {line_no}, col 1: expected assignment"))?.trim();
    let right = parts.next().ok_or_else(|| format!("line {line_no}, col 1: expected '='"))?.trim();
    if left.is_empty() || right.is_empty() { return Err(format!("line {line_no}, col 1: malformed assignment")); }
    Ok((left, right))
}

fn parse_expr(input: &str, line_no: usize) -> Result<Expr, String> {
    let tokens = tokenize(input, line_no)?;
    let (expr, pos) = parse_expr_bp(&tokens, 0, 0, line_no)?;
    if pos != tokens.len() { return Err(format!("line {line_no}, col 1: unexpected token `{}`", tokens[pos])); }
    Ok(expr)
}

fn parse_expr_bp(tokens: &[String], mut pos: usize, min_bp: u8, line_no: usize) -> Result<(Expr, usize), String> {
    let mut lhs = if pos >= tokens.len() { return Err(format!("line {line_no}, col 1: unexpected end of expression")); }
    else if tokens[pos] == "if" {
        pos += 1;
        let (cond, p1) = parse_expr_bp(tokens, pos, 0, line_no)?; pos = p1;
        let (then_src, next1) = extract_braced_from_tokens(tokens, pos, line_no)?; pos = next1;
        if tokens.get(pos).map(|s| s.as_str()) != Some("else") { return Err(format!("line {line_no}, col 1: expected `else`")); }
        pos += 1;
        let (else_src, next2) = extract_braced_from_tokens(tokens, pos, line_no)?; pos = next2;
        let then_block = Block { stmts: parse_statement_list(&split_logical_statements(&then_src)?)? };
        let else_block = Block { stmts: parse_statement_list(&split_logical_statements(&else_src)?)? };
        Expr::If { cond: Box::new(cond), then_branch: Box::new(then_block), else_branch: Box::new(else_block) }
    } else if tokens[pos] == "(" {
        pos += 1;
        let (first, next) = parse_expr_bp(tokens, pos, 0, line_no)?;
        pos = next;
        if tokens.get(pos).map(|s| s.as_str()) == Some(",") {
            let mut items = vec![first];
            while tokens.get(pos).map(|s| s.as_str()) == Some(",") { pos += 1; let (it, n2) = parse_expr_bp(tokens, pos, 0, line_no)?; items.push(it); pos = n2; }
            if tokens.get(pos).map(|s| s.as_str()) != Some(")") { return Err(format!("line {line_no}, col 1: missing ')' after tuple")); }
            pos += 1;
            Expr::Tuple(items)
        } else {
            if tokens.get(pos).map(|s| s.as_str()) != Some(")") { return Err(format!("line {line_no}, col 1: missing ')'")); }
            pos += 1;
            first
        }
    } else if tokens[pos].starts_with('"') && tokens[pos].ends_with('"') {
        let s = tokens[pos].trim_matches('"').to_string(); pos += 1; Expr::String(s)
    } else if let Ok(n) = tokens[pos].parse::<f64>() { pos += 1; Expr::Number(n)
    } else if tokens[pos] == "true" || tokens[pos] == "false" { let b=tokens[pos]=="true"; pos+=1; Expr::Bool(b)
    } else {
        let ident = tokens[pos].clone(); pos += 1;
        if tokens.get(pos).map(|s| s.as_str()) == Some("(") {
            pos += 1;
            let mut args = Vec::new();
            if tokens.get(pos).map(|s| s.as_str()) != Some(")") {
                loop {
                    let (arg, next) = parse_expr_bp(tokens, pos, 0, line_no)?;
                    args.push(arg);
                    pos = next;
                    match tokens.get(pos).map(|s| s.as_str()) { Some(",") => pos += 1, Some(")") => break, _ => return Err(format!("line {line_no}, col 1: expected ',' or ')' in call")), }
                }
            }
            if tokens.get(pos).map(|s| s.as_str()) != Some(")") { return Err(format!("line {line_no}, col 1: missing ')' after call")); }
            pos += 1;
            Expr::Call { name: ident, args }
        } else { Expr::Var(ident) }
    };

    while pos < tokens.len() {
        let op = match tokens[pos].as_str() { "+" => Op::Add, "-" => Op::Sub, "*" => Op::Mul, "/" => Op::Div, ")" | "," | "}" | "{" => break, _ => return Err(format!("line {line_no}, col 1: unknown operator `{}`", tokens[pos])), };
        let (l, r) = binding(op);
        if l < min_bp { break; }
        pos += 1;
        let (rhs, next) = parse_expr_bp(tokens, pos, r, line_no)?;
        pos = next;
        lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
    }
    Ok((lhs, pos))
}

fn extract_braced(input: &str, line_no: usize) -> Result<(String, usize), String> {
    let lb = input.find('{').ok_or_else(|| format!("line {line_no}, col 1: expected '{{'"))?;
    let rb = input.rfind('}').ok_or_else(|| format!("line {line_no}, col 1: expected '}}'"))?;
    if rb <= lb { return Err(format!("line {line_no}, col 1: malformed block")); }
    Ok((input[lb + 1..rb].trim().to_string(), rb + 1))
}

fn extract_braced_from_tokens(tokens: &[String], start: usize, line_no: usize) -> Result<(String, usize), String> {
    if tokens.get(start).map(|s| s.as_str()) != Some("{") { return Err(format!("line {line_no}, col 1: expected `{{`")); }
    let mut depth = 0i32;
    let mut i = start;
    let mut body = Vec::new();
    while i < tokens.len() {
        let t = &tokens[i];
        if t == "{" { depth += 1; if depth > 1 { body.push(t.clone()); } }
        else if t == "}" { depth -= 1; if depth == 0 { return Ok((body.join(" "), i + 1)); } body.push(t.clone()); }
        else { body.push(t.clone()); }
        i += 1;
    }
    Err(format!("line {line_no}, col 1: unterminated block"))
}

fn parse_pattern(tok: &str) -> Pattern {
    if tok == "_" { Pattern::Wildcard }
    else if tok == "true" || tok == "false" { Pattern::Bool(tok == "true") }
    else if let Ok(n) = tok.parse::<f64>() { Pattern::Number(n) }
    else { Pattern::Var(tok.to_string()) }
}

fn binding(op: Op) -> (u8, u8) { match op { Op::Add | Op::Sub => (1,2), Op::Mul | Op::Div => (3,4) } }

fn tokenize(input: &str, line_no: usize) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            ' ' | '\t' | '\n' | '\r' => { if !buf.is_empty() { tokens.push(std::mem::take(&mut buf)); } i += 1; }
            '"' => {
                if !buf.is_empty() { tokens.push(std::mem::take(&mut buf)); }
                let start = i; i += 1; let mut s = String::new();
                while i < chars.len() && chars[i] != '"' { s.push(chars[i]); i += 1; }
                if i >= chars.len() { return Err(format!("line {line_no}, col {}: unterminated string", start + 1)); }
                i += 1; tokens.push(format!("\"{}\"", s));
            }
            '=' if i + 1 < chars.len() && chars[i + 1] == '>' => { if !buf.is_empty() { tokens.push(std::mem::take(&mut buf)); } tokens.push("=>".into()); i += 2; }
            '(' | ')' | '+' | '-' | '*' | '/' | ',' | '|' | '{' | '}' => { if !buf.is_empty() { tokens.push(std::mem::take(&mut buf)); } tokens.push(ch.to_string()); i += 1; }
            c if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':' => { buf.push(c); i += 1; }
            _ => return Err(format!("line {line_no}, col {}: invalid character `{ch}`", i + 1)),
        }
    }
    if !buf.is_empty() { tokens.push(buf); }
    Ok(tokens)
}
