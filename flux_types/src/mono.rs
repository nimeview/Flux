//! Generic function monomorphization (v0.4).

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{HirExpr, HirFn, HirItem, HirParam, HirProgram, HirStmt, HirType, SymbolId};

use crate::types::Type;

pub fn monomorphize_program(
    program: &mut HirProgram,
    source: SourceId,
    diagnostics: &mut DiagnosticBag,
) {
    let mut generic_fns = program.generic_fns.clone();
    if generic_fns.is_empty() {
        for item in &program.items {
            if let HirItem::Fn(f) = item {
                if !f.type_params.is_empty() {
                    if let Some(name) = program.names.get(&f.name) {
                        generic_fns.insert(
                            name.clone(),
                            flux_hir::GenericFnTemplate {
                                type_params: f.type_params.clone(),
                                fn_item: f.clone(),
                            },
                        );
                    }
                }
            }
        }
    }
    if generic_fns.is_empty() {
        return;
    }

    let mut next_id = program.names.keys().map(|id| id.0).max().unwrap_or(0) + 1;

    let mut to_instantiate: std::collections::HashSet<String> = std::collections::HashSet::new();
    collect_generic_calls(program, &generic_fns, &mut to_instantiate);

    for base_name in to_instantiate {
        let Some(template) = generic_fns.get(&base_name) else {
            continue;
        };
        let calls = collect_all_call_sites(program, &base_name);
        let mut seen_sigs: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (args, span) in calls {
            let concrete: Vec<Type> = args.iter().map(infer_mono_type).collect();
            if concrete.len() != template.type_params.len() {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: format!("{} type argument(s)", template.type_params.len()),
                    found: format!("{} argument(s)", args.len()),
                    span: span_to_miette(span),
                    source_id: source,
                });
                continue;
            }
            let suffix = concrete
                .iter()
                .map(|t| t.display())
                .collect::<Vec<_>>()
                .join("_");
            if !seen_sigs.insert(suffix.clone()) {
                continue;
            }
            let inst_name = format!("{base_name}_{suffix}");
            if program.names.values().any(|n| n == &inst_name) {
                rewrite_matching_calls(program, &base_name, &inst_name, &concrete);
                continue;
            }
            let sym = SymbolId(next_id);
            next_id += 1;
            program.names.insert(sym, inst_name.clone());
            let mut subst: HashMap<&str, HirType> = HashMap::new();
            for (param, ty) in template.type_params.iter().zip(concrete.iter()) {
                subst.insert(param.as_str(), type_to_hir_type(ty, span));
            }
            let mut inst = instantiate_fn(&template.fn_item, sym, &subst);
            inst.type_params.clear();
            program.items.push(HirItem::Fn(inst));
            rewrite_matching_calls(program, &base_name, &inst_name, &concrete);
        }
    }
}

fn type_to_hir_type(ty: &Type, span: flux_diagnostics::Span) -> HirType {
    match ty {
        Type::Int => HirType::Named {
            name: "Int".into(),
            span,
        },
        Type::Float => HirType::Named {
            name: "Float".into(),
            span,
        },
        Type::Bool => HirType::Named {
            name: "Bool".into(),
            span,
        },
        Type::String => HirType::Named {
            name: "String".into(),
            span,
        },
        other => HirType::Named {
            name: other.display(),
            span,
        },
    }
}

fn infer_mono_type(expr: &HirExpr) -> Type {
    match expr {
        HirExpr::Int { .. } => Type::Int,
        HirExpr::Float { .. } => Type::Float,
        HirExpr::Bool { .. } => Type::Bool,
        HirExpr::String { .. } => Type::String,
        _ => Type::Int,
    }
}

fn substitute_hir_type(ty: &HirType, subst: &HashMap<&str, HirType>) -> HirType {
    match ty {
        HirType::TypeParam { name, span } => {
            subst
                .get(name.as_str())
                .cloned()
                .unwrap_or(HirType::TypeParam {
                    name: name.clone(),
                    span: *span,
                })
        }
        HirType::Named { name, span } if subst.contains_key(name.as_str()) => {
            subst.get(name.as_str()).cloned().unwrap()
        }
        HirType::Array { inner, span } => HirType::Array {
            inner: Box::new(substitute_hir_type(inner, subst)),
            span: *span,
        },
        HirType::Option { inner, span } => HirType::Option {
            inner: Box::new(substitute_hir_type(inner, subst)),
            span: *span,
        },
        HirType::Result { ok, err, span } => HirType::Result {
            ok: Box::new(substitute_hir_type(ok, subst)),
            err: Box::new(substitute_hir_type(err, subst)),
            span: *span,
        },
        other => other.clone(),
    }
}

fn instantiate_fn(template: &HirFn, name: SymbolId, subst: &HashMap<&str, HirType>) -> HirFn {
    HirFn {
        name,
        type_params: vec![],
        params: template
            .params
            .iter()
            .map(|p| HirParam {
                name: p.name,
                ty: substitute_hir_type(&p.ty, subst),
                span: p.span,
            })
            .collect(),
        ret: template.ret.as_ref().map(|t| substitute_hir_type(t, subst)),
        body: template.body.clone(),
        reactive: template.reactive,
        span: template.span,
    }
}

fn collect_generic_calls(
    program: &HirProgram,
    generic_fns: &HashMap<String, flux_hir::GenericFnTemplate>,
    out: &mut std::collections::HashSet<String>,
) {
    for item in &program.items {
        walk_item_calls(item, program, generic_fns, out);
    }
}

fn walk_item_calls(
    item: &HirItem,
    program: &HirProgram,
    generic_fns: &HashMap<String, flux_hir::GenericFnTemplate>,
    out: &mut std::collections::HashSet<String>,
) {
    match item {
        HirItem::Binding(b) => walk_expr_calls(&b.value, program, generic_fns, out),
        HirItem::Fn(f) => walk_block_calls(&f.body, program, generic_fns, out),
        HirItem::Watch(w) => walk_block_calls(&w.body, program, generic_fns, out),
        HirItem::On(o) => walk_block_calls(&o.body, program, generic_fns, out),
        HirItem::SourceAssign(a) => walk_expr_calls(&a.value, program, generic_fns, out),
        HirItem::InitIf(i) => walk_if_calls(i, program, generic_fns, out),
        _ => {}
    }
}

fn walk_if_calls(
    i: &flux_hir::HirIf,
    program: &HirProgram,
    generic_fns: &HashMap<String, flux_hir::GenericFnTemplate>,
    out: &mut std::collections::HashSet<String>,
) {
    walk_expr_calls(&i.cond, program, generic_fns, out);
    walk_block_calls(&i.then_branch, program, generic_fns, out);
    if let Some(e) = &i.else_branch {
        match e {
            flux_hir::HirElse::If(inner) => walk_if_calls(inner, program, generic_fns, out),
            flux_hir::HirElse::Block(b) => walk_block_calls(b, program, generic_fns, out),
        }
    }
}

fn walk_block_calls(
    block: &flux_hir::HirBlock,
    program: &HirProgram,
    generic_fns: &HashMap<String, flux_hir::GenericFnTemplate>,
    out: &mut std::collections::HashSet<String>,
) {
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Expr(e) => walk_expr_calls(e, program, generic_fns, out),
            HirStmt::Let(s) => walk_expr_calls(&s.value, program, generic_fns, out),
            HirStmt::Assign(a) => walk_expr_calls(&a.value, program, generic_fns, out),
            HirStmt::Return(r) => {
                if let Some(v) = &r.value {
                    walk_expr_calls(v, program, generic_fns, out);
                }
            }
            HirStmt::If(i) => walk_if_calls(i, program, generic_fns, out),
            HirStmt::While(w) => {
                walk_expr_calls(&w.cond, program, generic_fns, out);
                walk_block_calls(&w.body, program, generic_fns, out);
            }
            HirStmt::For(f) => {
                walk_expr_calls(&f.iter, program, generic_fns, out);
                walk_block_calls(&f.body, program, generic_fns, out);
            }
            HirStmt::Watch(w) => walk_block_calls(&w.body, program, generic_fns, out),
            HirStmt::On(o) => walk_block_calls(&o.body, program, generic_fns, out),
            HirStmt::Emit(e) => {
                for arg in &e.args {
                    walk_expr_calls(arg, program, generic_fns, out);
                }
            }
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn walk_expr_calls(
    expr: &HirExpr,
    program: &HirProgram,
    generic_fns: &HashMap<String, flux_hir::GenericFnTemplate>,
    out: &mut std::collections::HashSet<String>,
) {
    if let HirExpr::Call { callee, .. } = expr {
        if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
            if let Some(name) = program.names.get(symbol) {
                if generic_fns.contains_key(name) {
                    out.insert(name.clone());
                }
            }
        }
    }
    match expr {
        HirExpr::Call { callee, args, .. } => {
            walk_expr_calls(callee, program, generic_fns, out);
            for a in args {
                walk_expr_calls(a, program, generic_fns, out);
            }
        }
        HirExpr::Binary { left, right, .. } => {
            walk_expr_calls(left, program, generic_fns, out);
            walk_expr_calls(right, program, generic_fns, out);
        }
        HirExpr::Unary { expr, .. } => walk_expr_calls(expr, program, generic_fns, out),
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            walk_expr_calls(cond, program, generic_fns, out);
            walk_expr_calls(then_expr, program, generic_fns, out);
            walk_expr_calls(else_expr, program, generic_fns, out);
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            walk_expr_calls(scrutinee, program, generic_fns, out);
            for arm in arms {
                walk_expr_calls(&arm.expr, program, generic_fns, out);
            }
        }
        HirExpr::Field { base, .. } => walk_expr_calls(base, program, generic_fns, out),
        _ => {}
    }
}

fn collect_all_call_sites(
    program: &HirProgram,
    base_name: &str,
) -> Vec<(Vec<HirExpr>, flux_diagnostics::Span)> {
    let mut out = Vec::new();
    for item in &program.items {
        collect_calls_in_item(program, item, base_name, &mut out);
    }
    out
}

fn collect_calls_in_item(
    program: &HirProgram,
    item: &HirItem,
    base_name: &str,
    out: &mut Vec<(Vec<HirExpr>, flux_diagnostics::Span)>,
) {
    match item {
        HirItem::Binding(b) => collect_calls_in_expr(program, &b.value, base_name, out),
        HirItem::Fn(f) => collect_calls_in_block(program, &f.body, base_name, out),
        HirItem::Watch(w) => collect_calls_in_block(program, &w.body, base_name, out),
        HirItem::On(o) => collect_calls_in_block(program, &o.body, base_name, out),
        HirItem::SourceAssign(a) => collect_calls_in_expr(program, &a.value, base_name, out),
        HirItem::InitIf(i) => {
            collect_calls_in_expr(program, &i.cond, base_name, out);
            collect_calls_in_block(program, &i.then_branch, base_name, out);
            if let Some(e) = &i.else_branch {
                match e {
                    flux_hir::HirElse::If(inner) => {
                        collect_calls_in_expr(program, &inner.cond, base_name, out);
                        collect_calls_in_block(program, &inner.then_branch, base_name, out);
                    }
                    flux_hir::HirElse::Block(b) => {
                        collect_calls_in_block(program, b, base_name, out);
                    }
                }
            }
        }
        _ => {}
    }
}

fn collect_calls_in_block(
    program: &HirProgram,
    block: &flux_hir::HirBlock,
    base_name: &str,
    out: &mut Vec<(Vec<HirExpr>, flux_diagnostics::Span)>,
) {
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Expr(e) => collect_calls_in_expr(program, e, base_name, out),
            HirStmt::Let(s) => collect_calls_in_expr(program, &s.value, base_name, out),
            HirStmt::Assign(a) => collect_calls_in_expr(program, &a.value, base_name, out),
            HirStmt::Return(r) => {
                if let Some(v) = &r.value {
                    collect_calls_in_expr(program, v, base_name, out);
                }
            }
            HirStmt::If(i) => {
                collect_calls_in_expr(program, &i.cond, base_name, out);
                collect_calls_in_block(program, &i.then_branch, base_name, out);
                if let Some(e) = &i.else_branch {
                    match e {
                        flux_hir::HirElse::If(inner) => {
                            collect_calls_in_expr(program, &inner.cond, base_name, out);
                            collect_calls_in_block(program, &inner.then_branch, base_name, out);
                        }
                        flux_hir::HirElse::Block(b) => {
                            collect_calls_in_block(program, b, base_name, out);
                        }
                    }
                }
            }
            HirStmt::While(w) => {
                collect_calls_in_expr(program, &w.cond, base_name, out);
                collect_calls_in_block(program, &w.body, base_name, out);
            }
            HirStmt::For(f) => {
                collect_calls_in_expr(program, &f.iter, base_name, out);
                collect_calls_in_block(program, &f.body, base_name, out);
            }
            HirStmt::Watch(w) => collect_calls_in_block(program, &w.body, base_name, out),
            HirStmt::On(o) => collect_calls_in_block(program, &o.body, base_name, out),
            HirStmt::Emit(e) => {
                for arg in &e.args {
                    collect_calls_in_expr(program, arg, base_name, out);
                }
            }
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn collect_calls_in_expr(
    program: &HirProgram,
    expr: &HirExpr,
    base_name: &str,
    out: &mut Vec<(Vec<HirExpr>, flux_diagnostics::Span)>,
) {
    if let HirExpr::Call { callee, args, span } = expr {
        if let HirExpr::Ident { symbol, .. } = callee.as_ref() {
            if program.names.get(symbol).map(|n| n.as_str()) == Some(base_name) {
                out.push((args.clone(), *span));
            }
        }
    }
    match expr {
        HirExpr::Call { callee, args, .. } => {
            collect_calls_in_expr(program, callee, base_name, out);
            for a in args {
                collect_calls_in_expr(program, a, base_name, out);
            }
        }
        HirExpr::Binary { left, right, .. } => {
            collect_calls_in_expr(program, left, base_name, out);
            collect_calls_in_expr(program, right, base_name, out);
        }
        HirExpr::Unary { expr, .. } => collect_calls_in_expr(program, expr, base_name, out),
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_calls_in_expr(program, cond, base_name, out);
            collect_calls_in_expr(program, then_expr, base_name, out);
            collect_calls_in_expr(program, else_expr, base_name, out);
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            collect_calls_in_expr(program, scrutinee, base_name, out);
            for arm in arms {
                collect_calls_in_expr(program, &arm.expr, base_name, out);
            }
        }
        HirExpr::Field { base, .. } => collect_calls_in_expr(program, base, base_name, out),
        _ => {}
    }
}

fn call_matches_types(args: &[HirExpr], expected: &[Type]) -> bool {
    if args.len() != expected.len() {
        return false;
    }
    args.iter()
        .zip(expected.iter())
        .all(|(arg, ty)| infer_mono_type(arg) == *ty)
}

fn rewrite_matching_calls(program: &mut HirProgram, from: &str, to: &str, match_types: &[Type]) {
    let to_id = program
        .names
        .iter()
        .find(|(_, n)| n.as_str() == to)
        .map(|(id, _)| *id);
    let Some(to_id) = to_id else { return };
    let from_id = program
        .names
        .iter()
        .find(|(_, n)| n.as_str() == from)
        .map(|(id, _)| *id);
    let Some(from_id) = from_id else { return };

    for item in &mut program.items {
        rewrite_item_matching_calls(item, from_id, to_id, match_types);
    }
}

fn rewrite_item_matching_calls(
    item: &mut HirItem,
    from: SymbolId,
    to: SymbolId,
    match_types: &[Type],
) {
    match item {
        HirItem::Binding(b) => rewrite_expr_matching_calls(&mut b.value, from, to, match_types),
        HirItem::Fn(f) => rewrite_block_matching_calls(&mut f.body, from, to, match_types),
        HirItem::Watch(w) => rewrite_block_matching_calls(&mut w.body, from, to, match_types),
        HirItem::On(o) => rewrite_block_matching_calls(&mut o.body, from, to, match_types),
        HirItem::SourceAssign(a) => {
            rewrite_expr_matching_calls(&mut a.value, from, to, match_types)
        }
        HirItem::InitIf(i) => {
            rewrite_expr_matching_calls(&mut i.cond, from, to, match_types);
            rewrite_block_matching_calls(&mut i.then_branch, from, to, match_types);
            if let Some(e) = &mut i.else_branch {
                match e {
                    flux_hir::HirElse::If(inner) => {
                        rewrite_expr_matching_calls(&mut inner.cond, from, to, match_types);
                        rewrite_block_matching_calls(&mut inner.then_branch, from, to, match_types);
                    }
                    flux_hir::HirElse::Block(b) => {
                        rewrite_block_matching_calls(b, from, to, match_types);
                    }
                }
            }
        }
        _ => {}
    }
}

fn rewrite_block_matching_calls(
    block: &mut flux_hir::HirBlock,
    from: SymbolId,
    to: SymbolId,
    match_types: &[Type],
) {
    for stmt in &mut block.stmts {
        match stmt {
            HirStmt::Expr(e) => rewrite_expr_matching_calls(e, from, to, match_types),
            HirStmt::Let(s) => rewrite_expr_matching_calls(&mut s.value, from, to, match_types),
            HirStmt::Assign(a) => rewrite_expr_matching_calls(&mut a.value, from, to, match_types),
            HirStmt::Return(r) => {
                if let Some(v) = &mut r.value {
                    rewrite_expr_matching_calls(v, from, to, match_types);
                }
            }
            HirStmt::If(i) => {
                rewrite_expr_matching_calls(&mut i.cond, from, to, match_types);
                rewrite_block_matching_calls(&mut i.then_branch, from, to, match_types);
                if let Some(e) = &mut i.else_branch {
                    match e {
                        flux_hir::HirElse::If(inner) => {
                            rewrite_expr_matching_calls(&mut inner.cond, from, to, match_types);
                            rewrite_block_matching_calls(
                                &mut inner.then_branch,
                                from,
                                to,
                                match_types,
                            );
                        }
                        flux_hir::HirElse::Block(b) => {
                            rewrite_block_matching_calls(b, from, to, match_types);
                        }
                    }
                }
            }
            HirStmt::While(w) => {
                rewrite_expr_matching_calls(&mut w.cond, from, to, match_types);
                rewrite_block_matching_calls(&mut w.body, from, to, match_types);
            }
            HirStmt::For(f) => {
                rewrite_expr_matching_calls(&mut f.iter, from, to, match_types);
                rewrite_block_matching_calls(&mut f.body, from, to, match_types);
            }
            HirStmt::Watch(w) => rewrite_block_matching_calls(&mut w.body, from, to, match_types),
            HirStmt::On(o) => rewrite_block_matching_calls(&mut o.body, from, to, match_types),
            HirStmt::Emit(e) => {
                for arg in &mut e.args {
                    rewrite_expr_matching_calls(arg, from, to, match_types);
                }
            }
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn rewrite_expr_matching_calls(
    expr: &mut HirExpr,
    from: SymbolId,
    to: SymbolId,
    match_types: &[Type],
) {
    if let HirExpr::Call { callee, args, .. } = expr {
        if let HirExpr::Ident { symbol, .. } = callee.as_mut() {
            if *symbol == from && call_matches_types(args, match_types) {
                *symbol = to;
            }
        }
    }
    match expr {
        HirExpr::Call { callee, args, .. } => {
            rewrite_expr_matching_calls(callee, from, to, match_types);
            for a in args {
                rewrite_expr_matching_calls(a, from, to, match_types);
            }
        }
        HirExpr::Binary { left, right, .. } => {
            rewrite_expr_matching_calls(left, from, to, match_types);
            rewrite_expr_matching_calls(right, from, to, match_types);
        }
        HirExpr::Unary { expr: inner, .. } => {
            rewrite_expr_matching_calls(inner, from, to, match_types)
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            rewrite_expr_matching_calls(cond, from, to, match_types);
            rewrite_expr_matching_calls(then_expr, from, to, match_types);
            rewrite_expr_matching_calls(else_expr, from, to, match_types);
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            rewrite_expr_matching_calls(scrutinee, from, to, match_types);
            for arm in arms {
                rewrite_expr_matching_calls(&mut arm.expr, from, to, match_types);
            }
        }
        HirExpr::Field { base, .. } => rewrite_expr_matching_calls(base, from, to, match_types),
        _ => {}
    }
}
