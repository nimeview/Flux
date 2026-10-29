//! Flux source formatter (v0.5).

use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_syntax::{parse_program, Item, Stmt};

/// Format Flux source. Returns formatted text, or diagnostics on parse failure.
pub fn format_source(input: &str) -> Result<String, DiagnosticBag> {
    let mut diagnostics = DiagnosticBag::new();
    let Some(program) = parse_program(SourceId(0), input, &mut diagnostics) else {
        return Err(diagnostics);
    };
    let mut out = String::new();
    for (i, item) in program.items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        format_item(item, &mut out, 0);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn pub_prefix(pub_: bool) -> &'static str {
    if pub_ {
        "pub "
    } else {
        ""
    }
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("    ");
    }
}

fn format_item(item: &Item, out: &mut String, level: usize) {
    match item {
        Item::Import(i) => {
            indent(out, level);
            if i.symbols.is_empty() {
                out.push_str(&format!("import {}\n", i.path));
            } else {
                out.push_str(&format!(
                    "import {} {{ {} }}\n",
                    i.path,
                    i.symbols.join(", ")
                ));
            }
        }
        Item::TypeAlias(t) => {
            indent(out, level);
            let prefix = if t.pub_ { "pub " } else { "" };
            out.push_str(&format!(
                "{prefix}type {} = {}\n",
                t.name.name,
                type_name_fmt(&t.ty)
            ));
        }
        Item::Const(c) => {
            indent(out, level);
            let prefix = if c.pub_ { "pub " } else { "" };
            let ty =
                c.ty.as_ref()
                    .map(|t| format!(": {}", type_name_fmt(t)))
                    .unwrap_or_default();
            out.push_str(&format!(
                "{prefix}const {}{} = {}\n",
                c.name.name,
                ty,
                expr_fmt(&c.value)
            ));
        }
        Item::Struct(s) => {
            indent(out, level);
            let prefix = pub_prefix(s.pub_);
            let params = if s.type_params.is_empty() {
                String::new()
            } else {
                format!("[{}]", s.type_params.join(", "))
            };
            out.push_str(&format!("{prefix}struct {}{} {{\n", s.name.name, params));
            for f in &s.fields {
                indent(out, level + 1);
                out.push_str(&format!("{}: {},\n", f.name.name, type_name_fmt(&f.ty)));
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Item::Enum(e) => {
            indent(out, level);
            let prefix = pub_prefix(e.pub_);
            out.push_str(&format!("{prefix}enum {} {{\n", e.name.name));
            for v in &e.variants {
                indent(out, level + 1);
                match &v.payload {
                    Some(flux_syntax::EnumPayload::Tuple(t)) => {
                        out.push_str(&format!("{}({}),\n", v.name.name, type_name_fmt(t)));
                    }
                    Some(flux_syntax::EnumPayload::Struct(fields)) => {
                        let fs = fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n.name, type_name_fmt(t)))
                            .collect::<Vec<_>>()
                            .join(", ");
                        out.push_str(&format!("{} {{ {fs} }},\n", v.name.name));
                    }
                    None => out.push_str(&format!("{},\n", v.name.name)),
                }
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Item::Trait(t) => {
            indent(out, level);
            let prefix = pub_prefix(t.pub_);
            out.push_str(&format!("{prefix}trait {} {{\n", t.name.name));
            for m in &t.methods {
                indent(out, level + 1);
                let ret = m
                    .ret
                    .as_ref()
                    .map(|r| format!(" -> {}", type_name_fmt(r)))
                    .unwrap_or_default();
                let params = m
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name.name, type_name_fmt(&p.ty)))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("fn {}({}){};\n", m.name.name, params, ret));
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Item::Impl(i) => {
            indent(out, level);
            out.push_str(&format!(
                "impl {} for {} {{\n",
                i.trait_name.name,
                type_name_fmt(&i.target_type)
            ));
            for m in &i.methods {
                format_fn_like(
                    false,
                    &m.name.name,
                    &m.type_params,
                    &m.params,
                    m.ret.as_ref(),
                    &m.body,
                    out,
                    level + 1,
                    false,
                );
            }
            indent(out, level);
            out.push_str("}\n");
        }
        Item::Fn(f) => format_fn_like(
            f.pub_,
            &f.name.name,
            &f.type_params,
            &f.params,
            f.ret.as_ref(),
            &f.body,
            out,
            level,
            false,
        ),
        Item::RFn(f) => format_fn_like(
            f.pub_,
            &f.name.name,
            &f.type_params,
            &f.params,
            f.ret.as_ref(),
            &f.body,
            out,
            level,
            true,
        ),
        Item::Event(e) => {
            indent(out, level);
            let prefix = pub_prefix(e.pub_);
            let params = e
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name.name, type_name_fmt(&p.ty)))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{prefix}event {}({})\n", e.name.name, params));
        }
        Item::Derived(d) => {
            indent(out, level);
            let prefix = pub_prefix(d.pub_);
            let ty =
                d.ty.as_ref()
                    .map(|t| format!(": {}", type_name_fmt(t)))
                    .unwrap_or_default();
            out.push_str(&format!(
                "{prefix}derived {}{} = {}\n",
                let_pattern_fmt(&d.pattern),
                ty,
                expr_fmt(&d.value)
            ));
        }
        Item::Let(l) => {
            indent(out, level);
            let prefix = pub_prefix(l.pub_);
            let ty =
                l.ty.as_ref()
                    .map(|t| format!(": {}", type_name_fmt(t)))
                    .unwrap_or_default();
            let mut_kw = if l.mutable { "mut " } else { "" };
            out.push_str(&format!(
                "{prefix}let {}{}{} = {}\n",
                mut_kw,
                let_pattern_fmt(&l.pattern),
                ty,
                expr_fmt(&l.value)
            ));
        }
        Item::Watch(w) => {
            indent(out, level);
            let target = w
                .target
                .as_ref()
                .map(|t| format!(" {}", t.name))
                .unwrap_or_default();
            out.push_str(&format!("watch{target} {{\n"));
            format_block(&w.body, out, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Item::On(o) => {
            indent(out, level);
            let param = o
                .param
                .as_ref()
                .map(|p| format!("{} -> ", p.name))
                .unwrap_or_default();
            out.push_str(&format!("on {} {{ {param}{{\n", o.event.name));
            format_block(&o.body, out, level + 1);
            indent(out, level);
            out.push_str("} }\n");
        }
        Item::If(i) => format_if_stmt(i, out, level),
    }
}

#[allow(clippy::too_many_arguments)]
fn format_fn_like(
    pub_: bool,
    name: &str,
    type_params: &[String],
    params: &[flux_syntax::Param],
    ret: Option<&flux_syntax::TypeName>,
    body: &flux_syntax::Block,
    out: &mut String,
    level: usize,
    reactive: bool,
) {
    indent(out, level);
    let prefix = pub_prefix(pub_);
    let kw = if reactive { "rfn" } else { "fn" };
    let tparams = if type_params.is_empty() {
        String::new()
    } else {
        format!("[{}]", type_params.join(", "))
    };
    let ps = params
        .iter()
        .map(|p| format!("{}: {}", p.name.name, type_name_fmt(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");
    let ret = ret
        .map(|r| format!(" -> {}", type_name_fmt(r)))
        .unwrap_or_default();
    out.push_str(&format!("{prefix}{kw} {name}{tparams}({ps}){ret} {{\n"));
    format_block(body, out, level + 1);
    indent(out, level);
    out.push_str("}\n");
}

fn format_block(block: &flux_syntax::Block, out: &mut String, level: usize) {
    for stmt in &block.stmts {
        format_stmt(stmt, out, level);
    }
}

fn format_stmt(stmt: &Stmt, out: &mut String, level: usize) {
    match stmt {
        Stmt::Let(l) => {
            indent(out, level);
            let prefix = pub_prefix(l.pub_);
            let ty =
                l.ty.as_ref()
                    .map(|t| format!(": {}", type_name_fmt(t)))
                    .unwrap_or_default();
            let mut_kw = if l.mutable { "mut " } else { "" };
            out.push_str(&format!(
                "{prefix}let {}{}{} = {}\n",
                mut_kw,
                let_pattern_fmt(&l.pattern),
                ty,
                expr_fmt(&l.value)
            ));
        }
        Stmt::Assign(a) => {
            indent(out, level);
            out.push_str(&format!("{} = {}\n", a.name.name, expr_fmt(&a.value)));
        }
        Stmt::Return(r) => {
            indent(out, level);
            match &r.value {
                Some(v) => out.push_str(&format!("return {}\n", expr_fmt(v))),
                None => out.push_str("return\n"),
            }
        }
        Stmt::Break(_) => {
            indent(out, level);
            out.push_str("break\n");
        }
        Stmt::Continue(_) => {
            indent(out, level);
            out.push_str("continue\n");
        }
        Stmt::Emit(e) => {
            indent(out, level);
            let args = e.args.iter().map(expr_fmt).collect::<Vec<_>>().join(", ");
            out.push_str(&format!("emit {}({})\n", e.event.name, args));
        }
        Stmt::Expr(e) => {
            indent(out, level);
            out.push_str(&format!("{}\n", expr_fmt(e)));
        }
        Stmt::If(i) => format_if_stmt(i, out, level),
        Stmt::While(w) => {
            indent(out, level);
            out.push_str(&format!("while {} {{\n", expr_fmt(&w.cond)));
            format_block(&w.body, out, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::For(f) => {
            indent(out, level);
            out.push_str(&format!(
                "for {} in {} {{\n",
                f.name.name,
                expr_fmt(&f.iter)
            ));
            format_block(&f.body, out, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::Watch(w) => {
            indent(out, level);
            let target = w
                .target
                .as_ref()
                .map(|t| format!(" {}", t.name))
                .unwrap_or_default();
            out.push_str(&format!("watch{target} {{\n"));
            format_block(&w.body, out, level + 1);
            indent(out, level);
            out.push_str("}\n");
        }
        Stmt::On(o) => {
            indent(out, level);
            let param = o
                .param
                .as_ref()
                .map(|p| format!("{} -> ", p.name))
                .unwrap_or_default();
            out.push_str(&format!("on {} {{ {param}{{\n", o.event.name));
            format_block(&o.body, out, level + 1);
            indent(out, level);
            out.push_str("} }\n");
        }
    }
}

fn format_if_stmt(stmt: &flux_syntax::IfStmt, out: &mut String, level: usize) {
    indent(out, level);
    out.push_str(&format!("if {} {{\n", expr_fmt(&stmt.cond)));
    format_block(&stmt.then_branch, out, level + 1);
    indent(out, level);
    if let Some(else_b) = &stmt.else_branch {
        out.push_str("} else ");
        match else_b {
            flux_syntax::ElseBranch::If(inner) => {
                out.push_str(&format!("if {} {{\n", expr_fmt(&inner.cond)));
                format_block(&inner.then_branch, out, level + 1);
                indent(out, level);
                out.push_str("}\n");
            }
            flux_syntax::ElseBranch::Block(b) => {
                out.push_str("{\n");
                format_block(b, out, level + 1);
                indent(out, level);
                out.push_str("}\n");
            }
        }
    } else {
        out.push_str("}\n");
    }
}

fn type_name_fmt(ty: &flux_syntax::TypeName) -> String {
    match ty {
        flux_syntax::TypeName::Simple { name, .. } => name.clone(),
        flux_syntax::TypeName::Generic { name, args, .. } => {
            format!(
                "{}[{}]",
                name,
                args.iter()
                    .map(type_name_fmt)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        flux_syntax::TypeName::Array { inner, .. } => format!("[{}]", type_name_fmt(inner)),
        flux_syntax::TypeName::Option { inner, .. } => {
            format!("Option[{}]", type_name_fmt(inner))
        }
        flux_syntax::TypeName::Result { ok, err, .. } => {
            format!("Result[{}, {}]", type_name_fmt(ok), type_name_fmt(err))
        }
    }
}

fn expr_fmt(expr: &flux_syntax::Expr) -> String {
    match expr {
        flux_syntax::Expr::Int { value, .. } => value.to_string(),
        flux_syntax::Expr::Float { value, .. } => value.to_string(),
        flux_syntax::Expr::Bool { value, .. } => value.to_string(),
        flux_syntax::Expr::String { value, .. } => format!("\"{value}\""),
        flux_syntax::Expr::InterpolatedString { parts, .. } => {
            let mut out = String::from("\"");
            for part in parts {
                match part {
                    flux_syntax::StringPart::Text(text) => out.push_str(text),
                    flux_syntax::StringPart::Ident(name) => out.push_str(&format!("{{{name}}}")),
                }
            }
            out.push('"');
            out
        }
        flux_syntax::Expr::Ident { name, .. } => name.clone(),
        flux_syntax::Expr::Unary { op, expr, .. } => {
            let op = match op {
                flux_syntax::UnaryOp::Neg => "-",
                flux_syntax::UnaryOp::Not => "!",
            };
            format!("{op}{}", expr_fmt(expr))
        }
        flux_syntax::Expr::Binary {
            op, left, right, ..
        } => {
            let op = match op {
                flux_syntax::BinaryOp::Or => "||",
                flux_syntax::BinaryOp::And => "&&",
                flux_syntax::BinaryOp::Eq => "==",
                flux_syntax::BinaryOp::Ne => "!=",
                flux_syntax::BinaryOp::Lt => "<",
                flux_syntax::BinaryOp::Le => "<=",
                flux_syntax::BinaryOp::Gt => ">",
                flux_syntax::BinaryOp::Ge => ">=",
                flux_syntax::BinaryOp::Add => "+",
                flux_syntax::BinaryOp::Sub => "-",
                flux_syntax::BinaryOp::Mul => "*",
                flux_syntax::BinaryOp::Div => "/",
            };
            format!("{} {op} {}", expr_fmt(left), expr_fmt(right))
        }
        flux_syntax::Expr::Call { callee, args, .. } => {
            let args = args.iter().map(expr_fmt).collect::<Vec<_>>().join(", ");
            format!("{}({args})", expr_fmt(callee))
        }
        flux_syntax::Expr::Field { base, field, .. } => {
            format!("{}.{}", expr_fmt(base), field)
        }
        flux_syntax::Expr::Closure { params, body, .. } => {
            let ps = params
                .iter()
                .map(|p| match &p.ty {
                    Some(ty) => format!("{}: {}", p.name.name, type_name_fmt(ty)),
                    None => p.name.name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("|{ps}| {}", expr_fmt(body))
        }
        flux_syntax::Expr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            format!(
                "if {} {{ {} }} else {{ {} }}",
                expr_fmt(cond),
                expr_fmt(then_expr),
                expr_fmt(else_expr)
            )
        }
        flux_syntax::Expr::Array { elems, .. } => {
            let items = elems.iter().map(expr_fmt).collect::<Vec<_>>().join(", ");
            format!("[{items}]")
        }
        flux_syntax::Expr::StructLit { name, fields, .. } => {
            let fs = fields
                .iter()
                .map(|f| format!("{}: {}", f.name.name, expr_fmt(&f.value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {fs} }}", name.name)
        }
        flux_syntax::Expr::Match {
            scrutinee, arms, ..
        } => {
            let arms = arms
                .iter()
                .map(|a| {
                    let guard = a
                        .guard
                        .as_ref()
                        .map(|g| format!(" if {}", expr_fmt(g)))
                        .unwrap_or_default();
                    format!(
                        "{}{} => {}",
                        match_pattern_fmt(&a.pattern),
                        guard,
                        expr_fmt(&a.expr)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("match {} {{ {arms} }}", expr_fmt(scrutinee))
        }
        flux_syntax::Expr::Cast { expr, ty, .. } => {
            format!("{} as {ty}", expr_fmt(expr))
        }
    }
}

fn let_pattern_fmt(pattern: &flux_syntax::LetPattern) -> String {
    match pattern {
        flux_syntax::LetPattern::Ident(ident) => ident.name.clone(),
        flux_syntax::LetPattern::Struct { name, fields, .. } => {
            let fs = fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {fs} }}", name.name)
        }
    }
}

fn match_pattern_fmt(pattern: &flux_syntax::MatchPattern) -> String {
    match pattern {
        flux_syntax::MatchPattern::Simple(v) => v.name.clone(),
        flux_syntax::MatchPattern::Tuple {
            variant, bindings, ..
        } => {
            if bindings.is_empty() {
                variant.name.clone()
            } else {
                let bs = bindings
                    .iter()
                    .map(|b| b.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({bs})", variant.name)
            }
        }
        flux_syntax::MatchPattern::Struct {
            variant, fields, ..
        } => {
            let fs = fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ {fs} }}", variant.name)
        }
    }
}
