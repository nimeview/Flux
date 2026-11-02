//! Generic function monomorphization (v0.4).

use std::collections::HashMap;

use flux_diagnostics::{span_to_miette, DiagnosticBag, FluxError, SourceId};
use flux_hir::{
    HirExpr, HirFn, HirItem, HirParam, HirProgram, HirStmt, HirType, Symbol, SymbolId, SymbolKind,
};

use crate::types::Type;

/// Diagnostics take their [`SourceId`] from the offending span, so errors in
/// imported modules point at their own file; `_source` is the root's id.
pub fn monomorphize_program(
    program: &mut HirProgram,
    _source: SourceId,
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
    let mut reachable_names = std::collections::HashSet::new();
    collect_generic_calls(program, &generic_fns, &mut reachable_names);
    let mut generic_names: Vec<_> = reachable_names.into_iter().collect();
    generic_names.sort();

    // Each pass can expose calls inside newly specialized functions. Rebuild
    // symbol types and repeat until every reachable generic call is rewritten.
    // The cap protects against unbounded polymorphic recursion (for example,
    // a generic function that recursively wraps its type in another array).
    const MAX_MONOMORPHIZATION_PASSES: usize = 256;
    let mut specializations: HashMap<(String, String), String> = HashMap::new();
    for pass in 0..MAX_MONOMORPHIZATION_PASSES {
        let mut created_specialization = false;

        for base_name in &generic_names {
            let Some(template) = generic_fns.get(base_name) else {
                continue;
            };
            let symbol_types = collect_symbol_types(program);
            let calls = collect_all_call_sites(program, base_name);

            for (args, span) in calls {
                if args.len() != template.fn_item.params.len() {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: format!("{} arguments", template.fn_item.params.len()),
                        found: format!("{} argument(s)", args.len()),
                        span: span_to_miette(span),
                        source_id: span.source,
                    });
                    continue;
                }
                let inferred: Vec<Option<Type>> = args
                    .iter()
                    .map(|arg| infer_mono_type(arg, &symbol_types, program))
                    .collect();
                for (arg, ty) in args.iter().zip(inferred.iter()) {
                    if ty.is_none() {
                        diagnostics.push(FluxError::TypeMismatch {
                            expected: "a concrete, inferable generic argument type".into(),
                            found: "an expression whose type cannot be inferred here".into(),
                            span: span_to_miette(arg.span()),
                            source_id: arg.span().source,
                        });
                    }
                }
                let Some(concrete) = inferred.into_iter().collect::<Option<Vec<_>>>() else {
                    continue;
                };
                let Some(substitutions) = infer_generic_substitutions(
                    &template.fn_item,
                    &template.type_params,
                    &concrete,
                    program,
                ) else {
                    diagnostics.push(FluxError::TypeMismatch {
                        expected: format!(
                            "all generic parameters of `{base_name}` to be inferable from its parameters"
                        ),
                        found: "generic type arguments could not be unified".into(),
                        span: span_to_miette(span),
                        source_id: span.source,
                    });
                    continue;
                };
                let generic_types: Vec<Type> = template
                    .type_params
                    .iter()
                    .filter_map(|name| substitutions.get(name).cloned())
                    .collect();
                let suffix = generic_types
                    .iter()
                    .map(|ty| ty.display())
                    .collect::<Vec<_>>()
                    .join("_");
                let specialization_key = (base_name.clone(), suffix.clone());
                if let Some(inst_name) = specializations.get(&specialization_key) {
                    rewrite_matching_calls(program, base_name, inst_name, &concrete, &symbol_types);
                    continue;
                }
                let readable_name = format!("{base_name}_{suffix}");
                // A user may declare `id_Int` alongside `id[T]`. Generated
                // functions must never silently reuse that unrelated symbol.
                let inst_name = if program.names.values().any(|name| name == &readable_name) {
                    format!("\0flux_mono_{}_{readable_name}", next_id)
                } else {
                    readable_name
                };
                specializations.insert(specialization_key, inst_name.clone());

                let sym = SymbolId(next_id);
                next_id += 1;
                program.names.insert(sym, inst_name.clone());
                if let Some(template_symbol) = program.symbols.get(&template.fn_item.name).cloned()
                {
                    program.symbols.insert(
                        sym,
                        Symbol {
                            id: sym,
                            name: inst_name.clone(),
                            kind: SymbolKind::Function {
                                reactive: template.fn_item.reactive,
                            },
                            span: template_symbol.span,
                        },
                    );
                }
                let mut subst: HashMap<&str, HirType> = HashMap::new();
                for (param, ty) in template.type_params.iter().zip(generic_types.iter()) {
                    subst.insert(param.as_str(), type_to_hir_type(ty, span));
                }
                let mut inst = instantiate_fn(&template.fn_item, sym, &subst);
                inst.type_params.clear();
                remap_instantiated_symbols(&mut inst, &subst, program, &mut next_id);
                program.items.push(HirItem::Fn(inst));
                created_specialization = true;
                rewrite_matching_calls(program, base_name, &inst_name, &concrete, &symbol_types);
            }
        }

        if !created_specialization {
            break;
        }
        if pass + 1 == MAX_MONOMORPHIZATION_PASSES {
            if let Some(template) = generic_names.first().and_then(|name| generic_fns.get(name)) {
                diagnostics.push(FluxError::TypeMismatch {
                    expected: "finite generic specialization".into(),
                    found: "monomorphization exceeded 256 passes".into(),
                    span: span_to_miette(template.fn_item.span),
                    source_id: template.fn_item.span.source,
                });
            }
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
        Type::Array(inner) => HirType::Array {
            inner: Box::new(type_to_hir_type(inner, span)),
            span,
        },
        Type::Option(inner) => HirType::Option {
            inner: Box::new(type_to_hir_type(inner, span)),
            span,
        },
        Type::Result { ok, err } => HirType::Result {
            ok: Box::new(type_to_hir_type(ok, span)),
            err: Box::new(type_to_hir_type(err, span)),
            span,
        },
        Type::Struct { name, .. } | Type::Enum { name, .. } => HirType::Named {
            name: name.clone(),
            span,
        },
        Type::Func { params, ret } => HirType::Function {
            params: params.iter().map(|p| type_to_hir_type(p, span)).collect(),
            ret: Some(Box::new(type_to_hir_type(ret, span))),
            span,
        },
        other => HirType::Named {
            name: other.display(),
            span,
        },
    }
}

fn normalize_mono_type(ty: Type) -> Type {
    match ty {
        Type::Int64 => Type::Int,
        Type::Float64 => Type::Float,
        Type::Array(inner) => Type::Array(Box::new(normalize_mono_type(*inner))),
        Type::Option(inner) => Type::Option(Box::new(normalize_mono_type(*inner))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(normalize_mono_type(*ok)),
            err: Box::new(normalize_mono_type(*err)),
        },
        Type::Func { params, ret } => Type::Func {
            params: params.into_iter().map(normalize_mono_type).collect(),
            ret: Box::new(normalize_mono_type(*ret)),
        },
        other => other,
    }
}

fn infer_mono_type(
    expr: &HirExpr,
    symbols: &HashMap<SymbolId, Type>,
    program: &HirProgram,
) -> Option<Type> {
    match expr {
        HirExpr::Int { .. } => Some(Type::Int),
        HirExpr::Float { .. } => Some(Type::Float),
        HirExpr::Bool { .. } => Some(Type::Bool),
        HirExpr::String { .. } => Some(Type::String),
        HirExpr::Ident { symbol, .. } => symbols
            .get(symbol)
            .cloned()
            .or_else(|| function_type(symbol, symbols, program)),
        HirExpr::Snap { expr, .. } => infer_mono_type(expr, symbols, program),
        HirExpr::Array { elems, .. } => {
            let first = elems.first()?;
            let elem = infer_mono_type(first, symbols, program)?;
            elems
                .iter()
                .skip(1)
                .all(|e| infer_mono_type(e, symbols, program).as_ref() == Some(&elem))
                .then(|| Type::Array(Box::new(elem)))
        }
        HirExpr::Unary { op, expr, .. } => {
            let inner = infer_mono_type(expr, symbols, program)?;
            match op {
                flux_syntax::UnaryOp::Neg if inner.is_integer() || inner.is_float() => Some(inner),
                flux_syntax::UnaryOp::Not if inner == Type::Bool => Some(Type::Bool),
                _ => None,
            }
        }
        HirExpr::Binary {
            op, left, right, ..
        } => {
            use flux_syntax::BinaryOp;
            let left = infer_mono_type(left, symbols, program)?;
            let right = infer_mono_type(right, symbols, program)?;
            match op {
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or => Some(Type::Bool),
                BinaryOp::Add if left == Type::String && right == Type::String => {
                    Some(Type::String)
                }
                _ if left == right => Some(left),
                _ => None,
            }
        }
        HirExpr::If {
            then_expr,
            else_expr,
            ..
        } => {
            let then_ty = infer_mono_type(then_expr, symbols, program)?;
            let else_ty = infer_mono_type(else_expr, symbols, program)?;
            (then_ty == else_ty).then_some(then_ty)
        }
        HirExpr::Cast { ty, .. } => Some(normalize_mono_type(resolve_type_name(ty, program)?)),
        HirExpr::ToText { .. } => Some(Type::String),
        HirExpr::Index { base, .. } => match infer_mono_type(base, symbols, program)? {
            Type::Array(inner) => Some(*inner),
            _ => None,
        },
        HirExpr::Try { expr, .. } => match infer_mono_type(expr, symbols, program)? {
            Type::Result { ok, .. } => Some(*ok),
            Type::Option(inner) => Some(*inner),
            _ => None,
        },
        HirExpr::StructLit { name, .. } => resolve_type_name(name, program),
        HirExpr::Field { base, field, .. } => {
            let base = match infer_mono_type(base, symbols, program)? {
                // The end of a recursive type's expansion.
                Type::Enum { name, variants } if variants.is_empty() => {
                    resolve_type_name(&name, program)?
                }
                other => other,
            };
            let Type::Struct { fields, .. } = base else {
                return None;
            };
            fields
                .into_iter()
                .find_map(|(name, ty)| (name == *field).then_some(ty))
        }
        HirExpr::Match { arms, .. } => {
            let mut result = None;
            for arm in arms {
                let ty = infer_mono_type(&arm.expr, symbols, program)?;
                if result.as_ref().is_some_and(|found| found != &ty) {
                    return None;
                }
                result = Some(ty);
            }
            result
        }
        HirExpr::EnumVariant { enum_name, .. } => resolve_type_name(enum_name, program),
        HirExpr::Call { callee, args, .. } => {
            let HirExpr::Ident { symbol, .. } = callee.as_ref() else {
                return None;
            };
            let name = program.names.get(symbol).map(String::as_str)?;
            match name {
                "Some" => Some(Type::Option(Box::new(infer_mono_type(
                    args.first()?,
                    symbols,
                    program,
                )?))),
                "None" => Some(Type::Option(Box::new(Type::Unit))),
                "Ok" => Some(Type::Result {
                    ok: Box::new(infer_mono_type(args.first()?, symbols, program)?),
                    err: Box::new(Type::String),
                }),
                "Err" => Some(Type::Result {
                    ok: Box::new(Type::Unit),
                    err: Box::new(infer_mono_type(args.first()?, symbols, program)?),
                }),
                "range" => Some(Type::Array(Box::new(Type::Int))),
                _ => program.items.iter().find_map(|item| match item {
                    HirItem::Fn(function)
                        if function.name == *symbol && function.type_params.is_empty() =>
                    {
                        function
                            .ret
                            .as_ref()
                            .and_then(|ret| hir_type_to_mono_type(ret, program))
                            .or_else(|| match function.body.stmts.last() {
                                Some(HirStmt::Expr(expr)) => {
                                    infer_mono_type(expr, symbols, program)
                                }
                                Some(HirStmt::Return(ret)) => ret
                                    .value
                                    .as_ref()
                                    .and_then(|expr| infer_mono_type(expr, symbols, program)),
                                _ => None,
                            })
                    }
                    _ => None,
                }),
            }
        }
        HirExpr::Closure { fn_symbol, .. } => function_type(fn_symbol, symbols, program),
    }
}

/// The type of the declared function `symbol`, as a value.
fn function_type(
    symbol: &SymbolId,
    symbols: &HashMap<SymbolId, Type>,
    program: &HirProgram,
) -> Option<Type> {
    program.items.iter().find_map(|item| match item {
        HirItem::Fn(function) if function.name == *symbol && function.type_params.is_empty() => {
            let params = function
                .params
                .iter()
                .map(|p| hir_type_to_mono_type(&p.ty, program))
                .collect::<Option<Vec<_>>>()?;
            let ret = match &function.ret {
                Some(ret) => hir_type_to_mono_type(ret, program)?,
                None => match function.body.stmts.last() {
                    Some(HirStmt::Expr(expr)) => infer_mono_type(expr, symbols, program)?,
                    _ => Type::Unit,
                },
            };
            Some(Type::Func {
                params,
                ret: Box::new(ret),
            })
        }
        _ => None,
    })
}

fn resolve_type_name(name: &str, program: &HirProgram) -> Option<Type> {
    resolve_named(name, program, &mut Vec::new())
}

fn hir_type_to_mono_type(ty: &HirType, program: &HirProgram) -> Option<Type> {
    hir_to_mono(ty, program, &mut Vec::new())
}

/// `expanding`: the named types being expanded. A recursive reference to one
/// stays a placeholder `Enum { name, variants: [] }`, or this would not end.
fn resolve_named(name: &str, program: &HirProgram, expanding: &mut Vec<String>) -> Option<Type> {
    if let Ok(ty) =
        crate::types::resolve_type_name(name, flux_diagnostics::Span::new(SourceId(0), 0, 0))
    {
        return Some(normalize_mono_type(ty));
    }
    if expanding.iter().any(|open| open == name) {
        return Some(Type::Enum {
            name: name.into(),
            variants: vec![],
        });
    }
    expanding.push(name.into());
    let found = program
        .items
        .iter()
        .find_map(|item| match item {
            HirItem::Struct(s) if program.names.get(&s.name).map(String::as_str) == Some(name) => {
                Some(Type::Struct {
                    name: name.into(),
                    fields: s
                        .fields
                        .iter()
                        .filter_map(|(field, ty)| {
                            Some((field.clone(), hir_to_mono(ty, program, expanding)?))
                        })
                        .collect(),
                })
            }
            HirItem::Enum(e) if program.names.get(&e.name).map(String::as_str) == Some(name) => {
                Some(Type::Enum {
                    name: name.into(),
                    variants: e
                        .variants
                        .iter()
                        .map(|(variant, payload)| {
                            (
                                variant.clone(),
                                payload.as_ref().and_then(|payload| match payload {
                                    flux_hir::HirEnumPayload::Tuple(ty) => {
                                        hir_to_mono(ty, program, expanding)
                                    }
                                    flux_hir::HirEnumPayload::Struct(_) => None,
                                }),
                            )
                        })
                        .collect(),
                })
            }
            _ => None,
        })
        .or_else(|| {
            program
                .type_aliases
                .get(name)
                .and_then(|aliased| hir_to_mono(aliased, program, expanding))
        });
    expanding.pop();
    found
}

fn hir_to_mono(ty: &HirType, program: &HirProgram, expanding: &mut Vec<String>) -> Option<Type> {
    match ty {
        HirType::Named { name, .. } => resolve_named(name, program, expanding),
        HirType::TypeParam { .. } => None,
        HirType::Array { inner, .. } => Some(Type::Array(Box::new(hir_to_mono(
            inner, program, expanding,
        )?))),
        HirType::Option { inner, .. } => Some(Type::Option(Box::new(hir_to_mono(
            inner, program, expanding,
        )?))),
        HirType::Result { ok, err, .. } => Some(Type::Result {
            ok: Box::new(hir_to_mono(ok, program, expanding)?),
            err: Box::new(hir_to_mono(err, program, expanding)?),
        }),
        HirType::Generic { name, args, .. } if name == "Option" && args.len() == 1 => Some(
            Type::Option(Box::new(hir_to_mono(&args[0], program, expanding)?)),
        ),
        HirType::Generic { name, args, .. } if name == "Result" && args.len() == 2 => {
            Some(Type::Result {
                ok: Box::new(hir_to_mono(&args[0], program, expanding)?),
                err: Box::new(hir_to_mono(&args[1], program, expanding)?),
            })
        }
        HirType::Function { params, ret, .. } => Some(Type::Func {
            params: params
                .iter()
                .map(|p| hir_to_mono(p, program, expanding))
                .collect::<Option<Vec<_>>>()?,
            ret: Box::new(match ret {
                Some(ret) => hir_to_mono(ret, program, expanding)?,
                None => Type::Unit,
            }),
        }),
        HirType::Generic { name, .. } => resolve_named(name, program, expanding),
    }
}

fn infer_generic_substitutions(
    function: &HirFn,
    type_params: &[String],
    actual_types: &[Type],
    program: &HirProgram,
) -> Option<HashMap<String, Type>> {
    if function.params.len() != actual_types.len() {
        return None;
    }
    let mut substitutions = HashMap::new();
    for (param, actual) in function.params.iter().zip(actual_types) {
        unify_generic_type(&param.ty, actual, type_params, program, &mut substitutions)?;
    }
    type_params
        .iter()
        .all(|param| substitutions.contains_key(param))
        .then_some(substitutions)
}

fn unify_generic_type(
    declared: &HirType,
    actual: &Type,
    type_params: &[String],
    program: &HirProgram,
    substitutions: &mut HashMap<String, Type>,
) -> Option<()> {
    let parameter_name = match declared {
        HirType::TypeParam { name, .. } => Some(name),
        HirType::Named { name, .. } if type_params.iter().any(|param| param == name) => Some(name),
        _ => None,
    };
    if let Some(name) = parameter_name {
        let actual = normalize_mono_type(actual.clone());
        if let Some(previous) = substitutions.get(name) {
            return (previous == &actual).then_some(());
        }
        substitutions.insert(name.clone(), actual);
        return Some(());
    }

    match (declared, actual) {
        (HirType::Array { inner, .. }, Type::Array(actual_inner))
        | (HirType::Option { inner, .. }, Type::Option(actual_inner)) => {
            unify_generic_type(inner, actual_inner, type_params, program, substitutions)
        }
        (
            HirType::Result { ok, err, .. },
            Type::Result {
                ok: actual_ok,
                err: actual_err,
            },
        ) => {
            unify_generic_type(ok, actual_ok, type_params, program, substitutions)?;
            unify_generic_type(err, actual_err, type_params, program, substitutions)
        }
        (
            HirType::Function { params, ret, .. },
            Type::Func {
                params: actual_params,
                ret: actual_ret,
            },
        ) if params.len() == actual_params.len() => {
            for (param, actual) in params.iter().zip(actual_params) {
                unify_generic_type(param, actual, type_params, program, substitutions)?;
            }
            match ret {
                Some(ret) => {
                    unify_generic_type(ret, actual_ret, type_params, program, substitutions)
                }
                None => (**actual_ret == Type::Unit).then_some(()),
            }
        }
        (HirType::Generic { name, args, .. }, Type::Option(actual_inner))
            if name == "Option" && args.len() == 1 =>
        {
            unify_generic_type(&args[0], actual_inner, type_params, program, substitutions)
        }
        (HirType::Generic { name, args, .. }, Type::Result { ok, err })
            if name == "Result" && args.len() == 2 =>
        {
            unify_generic_type(&args[0], ok, type_params, program, substitutions)?;
            unify_generic_type(&args[1], err, type_params, program, substitutions)
        }
        _ => {
            let declared = hir_type_to_mono_type(declared, program)?;
            (normalize_mono_type(declared) == normalize_mono_type(actual.clone())).then_some(())
        }
    }
}

fn remap_instantiated_symbols(
    function: &mut HirFn,
    subst: &HashMap<&str, HirType>,
    program: &mut HirProgram,
    next_id: &mut u32,
) {
    let mut declared = std::collections::HashSet::new();
    for param in &function.params {
        declared.insert(param.name);
    }
    collect_block_local_declarations(&function.body, &mut declared);

    let mut remap = HashMap::new();
    for id in declared {
        let Some(symbol) = program.symbols.get(&id).cloned() else {
            continue;
        };
        if !matches!(symbol.kind, SymbolKind::Local { .. } | SymbolKind::Param) {
            continue;
        }
        let new_id = SymbolId(*next_id);
        *next_id += 1;
        let new_symbol = Symbol {
            id: new_id,
            ..symbol.clone()
        };
        program.names.insert(new_id, symbol.name);
        program.symbols.insert(new_id, new_symbol);
        remap.insert(id, new_id);
    }
    remap_function_symbols(
        function,
        &Remap {
            symbols: &remap,
            types: subst,
        },
    );
}

/// What a generic function's instance changes in its copy of the body: fresh
/// symbols for its locals, and concrete types for its type parameters.
struct Remap<'a> {
    symbols: &'a HashMap<SymbolId, SymbolId>,
    types: &'a HashMap<&'a str, HirType>,
}

fn collect_block_local_declarations(
    block: &flux_hir::HirBlock,
    out: &mut std::collections::HashSet<SymbolId>,
) {
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let(binding) => {
                out.insert(binding.name);
                collect_expr_local_declarations(&binding.value, out);
            }
            HirStmt::Assign(assign) => collect_expr_local_declarations(&assign.value, out),
            HirStmt::Emit(emit) => {
                for arg in &emit.args {
                    collect_expr_local_declarations(arg, out);
                }
            }
            HirStmt::Expr(expr) => collect_expr_local_declarations(expr, out),
            HirStmt::Return(ret) => {
                if let Some(expr) = &ret.value {
                    collect_expr_local_declarations(expr, out);
                }
            }
            HirStmt::If(if_stmt) => {
                collect_if_local_declarations(if_stmt, out);
            }
            HirStmt::While(loop_stmt) => {
                collect_expr_local_declarations(&loop_stmt.cond, out);
                collect_block_local_declarations(&loop_stmt.body, out);
            }
            HirStmt::For(loop_stmt) => {
                out.insert(loop_stmt.name);
                collect_expr_local_declarations(&loop_stmt.iter, out);
                collect_block_local_declarations(&loop_stmt.body, out);
            }
            HirStmt::Watch(watch) => collect_block_local_declarations(&watch.body, out),
            HirStmt::On(handler) => {
                if let Some(param) = handler.param {
                    out.insert(param);
                }
                collect_block_local_declarations(&handler.body, out);
            }
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn collect_if_local_declarations(
    if_stmt: &flux_hir::HirIf,
    out: &mut std::collections::HashSet<SymbolId>,
) {
    collect_expr_local_declarations(&if_stmt.cond, out);
    collect_block_local_declarations(&if_stmt.then_branch, out);
    if let Some(else_branch) = &if_stmt.else_branch {
        match else_branch {
            flux_hir::HirElse::If(nested) => collect_if_local_declarations(nested, out),
            flux_hir::HirElse::Block(block) => collect_block_local_declarations(block, out),
        }
    }
}

fn collect_expr_local_declarations(expr: &HirExpr, out: &mut std::collections::HashSet<SymbolId>) {
    match expr {
        HirExpr::Snap { expr, .. }
        | HirExpr::Unary { expr, .. }
        | HirExpr::Cast { expr, .. }
        | HirExpr::ToText { expr, .. }
        | HirExpr::Try { expr, .. } => collect_expr_local_declarations(expr, out),
        HirExpr::Call { callee, args, .. } => {
            collect_expr_local_declarations(callee, out);
            for arg in args {
                collect_expr_local_declarations(arg, out);
            }
        }
        HirExpr::Field { base, .. } => collect_expr_local_declarations(base, out),
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
            collect_expr_local_declarations(left, out);
            collect_expr_local_declarations(right, out);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_expr_local_declarations(cond, out);
            collect_expr_local_declarations(then_expr, out);
            collect_expr_local_declarations(else_expr, out);
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                collect_expr_local_declarations(elem, out);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, expr) in fields {
                collect_expr_local_declarations(expr, out);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            collect_expr_local_declarations(scrutinee, out);
            for arm in arms {
                out.extend(arm.bindings.iter().copied());
                if let Some(guard) = &arm.guard {
                    collect_expr_local_declarations(guard, out);
                }
                collect_block_local_declarations(&arm.body, out);
                collect_expr_local_declarations(&arm.expr, out);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(payload) = payload {
                collect_expr_local_declarations(payload, out);
            }
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. }
        | HirExpr::Ident { .. }
        | HirExpr::Closure { .. } => {}
    }
}

fn remap_function_symbols(function: &mut HirFn, remap: &Remap<'_>) {
    for param in &mut function.params {
        remap_id(&mut param.name, remap);
    }
    remap_block_symbols(&mut function.body, remap);
}

fn remap_id(id: &mut SymbolId, remap: &Remap<'_>) {
    if let Some(mapped) = remap.symbols.get(id) {
        *id = *mapped;
    }
}

fn remap_block_symbols(block: &mut flux_hir::HirBlock, remap: &Remap<'_>) {
    for stmt in &mut block.stmts {
        match stmt {
            HirStmt::Let(binding) => {
                remap_id(&mut binding.name, remap);
                if let Some(ty) = &mut binding.ty {
                    *ty = substitute_hir_type(ty, remap.types);
                }
                remap_expr_symbols(&mut binding.value, remap);
            }
            HirStmt::Assign(assign) => {
                remap_id(&mut assign.name, remap);
                remap_expr_symbols(&mut assign.value, remap);
            }
            HirStmt::Emit(emit) => {
                for arg in &mut emit.args {
                    remap_expr_symbols(arg, remap);
                }
            }
            HirStmt::Expr(expr) => remap_expr_symbols(expr, remap),
            HirStmt::Return(ret) => {
                if let Some(expr) = &mut ret.value {
                    remap_expr_symbols(expr, remap);
                }
            }
            HirStmt::If(if_stmt) => {
                remap_if_symbols(if_stmt, remap);
            }
            HirStmt::While(loop_stmt) => {
                remap_expr_symbols(&mut loop_stmt.cond, remap);
                remap_block_symbols(&mut loop_stmt.body, remap);
            }
            HirStmt::For(loop_stmt) => {
                remap_id(&mut loop_stmt.name, remap);
                remap_expr_symbols(&mut loop_stmt.iter, remap);
                remap_block_symbols(&mut loop_stmt.body, remap);
            }
            HirStmt::Watch(watch) => remap_block_symbols(&mut watch.body, remap),
            HirStmt::On(handler) => {
                if let Some(param) = &mut handler.param {
                    remap_id(param, remap);
                }
                remap_block_symbols(&mut handler.body, remap);
            }
            HirStmt::Break(_) | HirStmt::Continue(_) => {}
        }
    }
}

fn remap_if_symbols(if_stmt: &mut flux_hir::HirIf, remap: &Remap<'_>) {
    remap_expr_symbols(&mut if_stmt.cond, remap);
    remap_block_symbols(&mut if_stmt.then_branch, remap);
    if let Some(else_branch) = &mut if_stmt.else_branch {
        match else_branch {
            flux_hir::HirElse::If(nested) => remap_if_symbols(nested, remap),
            flux_hir::HirElse::Block(block) => remap_block_symbols(block, remap),
        }
    }
}

fn remap_expr_symbols(expr: &mut HirExpr, remap: &Remap<'_>) {
    match expr {
        HirExpr::Ident { symbol, .. } => remap_id(symbol, remap),
        HirExpr::Snap { expr, .. }
        | HirExpr::Unary { expr, .. }
        | HirExpr::Cast { expr, .. }
        | HirExpr::ToText { expr, .. }
        | HirExpr::Try { expr, .. } => remap_expr_symbols(expr, remap),
        HirExpr::Call { callee, args, .. } => {
            remap_expr_symbols(callee, remap);
            for arg in args {
                remap_expr_symbols(arg, remap);
            }
        }
        HirExpr::Field { base, .. } => remap_expr_symbols(base, remap),
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
            remap_expr_symbols(left, remap);
            remap_expr_symbols(right, remap);
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            remap_expr_symbols(cond, remap);
            remap_expr_symbols(then_expr, remap);
            remap_expr_symbols(else_expr, remap);
        }
        HirExpr::Array { elems, .. } => {
            for elem in elems {
                remap_expr_symbols(elem, remap);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, expr) in fields {
                remap_expr_symbols(expr, remap);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            remap_expr_symbols(scrutinee, remap);
            for arm in arms {
                for binding in &mut arm.bindings {
                    remap_id(binding, remap);
                }
                if let Some(guard) = &mut arm.guard {
                    remap_expr_symbols(guard, remap);
                }
                remap_block_symbols(&mut arm.body, remap);
                remap_expr_symbols(&mut arm.expr, remap);
            }
        }
        HirExpr::EnumVariant { payload, .. } => {
            if let Some(payload) = payload {
                remap_expr_symbols(payload, remap);
            }
        }
        HirExpr::Int { .. }
        | HirExpr::Float { .. }
        | HirExpr::Bool { .. }
        | HirExpr::String { .. } => {}
        // Its body keeps its own symbols; only what it loads here is this function's.
        HirExpr::Closure { captures, .. } => {
            for capture in captures {
                remap_id(capture, remap);
            }
        }
    }
}

fn collect_symbol_types(program: &HirProgram) -> HashMap<SymbolId, Type> {
    let mut symbols = HashMap::new();
    for _ in 0..=program.items.len() {
        let before = symbols.len();
        for item in &program.items {
            match item {
                HirItem::Binding(binding) => {
                    let ty = binding
                        .ty
                        .as_ref()
                        .and_then(|ty| hir_type_to_mono_type(ty, program))
                        .or_else(|| infer_mono_type(&binding.value, &symbols, program));
                    if let Some(ty) = ty {
                        symbols.insert(binding.name, ty);
                    }
                }
                HirItem::Fn(function) => {
                    for param in &function.params {
                        if let Some(ty) = hir_type_to_mono_type(&param.ty, program) {
                            symbols.insert(param.name, ty);
                        }
                    }
                    collect_block_symbol_types(&function.body, &mut symbols, program);
                }
                HirItem::Watch(watch) => {
                    collect_block_symbol_types(&watch.body, &mut symbols, program)
                }
                HirItem::On(handler) => {
                    collect_block_symbol_types(&handler.body, &mut symbols, program)
                }
                _ => {}
            }
        }
        if symbols.len() == before {
            break;
        }
    }
    symbols
}

fn collect_block_symbol_types(
    block: &flux_hir::HirBlock,
    symbols: &mut HashMap<SymbolId, Type>,
    program: &HirProgram,
) {
    for stmt in &block.stmts {
        match stmt {
            HirStmt::Let(binding) => {
                if let Some(ty) = binding
                    .ty
                    .as_ref()
                    .and_then(|ty| hir_type_to_mono_type(ty, program))
                    .or_else(|| infer_mono_type(&binding.value, symbols, program))
                {
                    symbols.insert(binding.name, ty);
                }
            }
            HirStmt::If(if_stmt) => {
                collect_block_symbol_types(&if_stmt.then_branch, symbols, program);
                if let Some(else_branch) = &if_stmt.else_branch {
                    match else_branch {
                        flux_hir::HirElse::If(nested) => {
                            collect_block_symbol_types(&nested.then_branch, symbols, program)
                        }
                        flux_hir::HirElse::Block(block) => {
                            collect_block_symbol_types(block, symbols, program)
                        }
                    }
                }
            }
            HirStmt::While(loop_stmt) => {
                collect_block_symbol_types(&loop_stmt.body, symbols, program)
            }
            HirStmt::For(loop_stmt) => {
                if let Some(Type::Array(inner)) = infer_mono_type(&loop_stmt.iter, symbols, program)
                {
                    symbols.insert(loop_stmt.name, *inner);
                }
                collect_block_symbol_types(&loop_stmt.body, symbols, program);
            }
            HirStmt::Watch(watch) => collect_block_symbol_types(&watch.body, symbols, program),
            HirStmt::On(handler) => collect_block_symbol_types(&handler.body, symbols, program),
            _ => {}
        }
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
        HirType::Generic { name, args, span } => HirType::Generic {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| substitute_hir_type(arg, subst))
                .collect(),
            span: *span,
        },
        HirType::Function { params, ret, span } => HirType::Function {
            params: params
                .iter()
                .map(|p| substitute_hir_type(p, subst))
                .collect(),
            ret: ret
                .as_ref()
                .map(|r| Box::new(substitute_hir_type(r, subst))),
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
        captures: template.captures.clone(),
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
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
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
                if let Some(guard) = &arm.guard {
                    walk_expr_calls(guard, program, generic_fns, out);
                }
                walk_block_calls(&arm.body, program, generic_fns, out);
                walk_expr_calls(&arm.expr, program, generic_fns, out);
            }
        }
        HirExpr::Field { base, .. } => walk_expr_calls(base, program, generic_fns, out),
        HirExpr::Snap { expr: inner, .. }
        | HirExpr::Cast { expr: inner, .. }
        | HirExpr::ToText { expr: inner, .. }
        | HirExpr::Try { expr: inner, .. } => {
            walk_expr_calls(inner, program, generic_fns, out);
        }
        HirExpr::Array { elems, .. } => {
            for e in elems {
                walk_expr_calls(e, program, generic_fns, out);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, e) in fields {
                walk_expr_calls(e, program, generic_fns, out);
            }
        }
        HirExpr::EnumVariant {
            payload: Some(p), ..
        } => {
            walk_expr_calls(p, program, generic_fns, out);
        }
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
        HirItem::Fn(f) if !f.type_params.is_empty() => {}
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
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
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
                if let Some(guard) = &arm.guard {
                    collect_calls_in_expr(program, guard, base_name, out);
                }
                collect_calls_in_block(program, &arm.body, base_name, out);
                collect_calls_in_expr(program, &arm.expr, base_name, out);
            }
        }
        HirExpr::Field { base, .. } => collect_calls_in_expr(program, base, base_name, out),
        HirExpr::Snap { expr: inner, .. }
        | HirExpr::Cast { expr: inner, .. }
        | HirExpr::ToText { expr: inner, .. }
        | HirExpr::Try { expr: inner, .. } => {
            collect_calls_in_expr(program, inner, base_name, out);
        }
        HirExpr::Array { elems, .. } => {
            for e in elems {
                collect_calls_in_expr(program, e, base_name, out);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, e) in fields {
                collect_calls_in_expr(program, e, base_name, out);
            }
        }
        HirExpr::EnumVariant {
            payload: Some(p), ..
        } => {
            collect_calls_in_expr(program, p, base_name, out);
        }
        _ => {}
    }
}

fn call_matches_types(
    args: &[HirExpr],
    expected: &[Type],
    symbols: &HashMap<SymbolId, Type>,
    program: &HirProgram,
) -> bool {
    if args.len() != expected.len() {
        return false;
    }
    args.iter()
        .zip(expected.iter())
        .all(|(arg, ty)| infer_mono_type(arg, symbols, program).as_ref() == Some(ty))
}

fn rewrite_matching_calls(
    program: &mut HirProgram,
    from: &str,
    to: &str,
    match_types: &[Type],
    symbol_types: &HashMap<SymbolId, Type>,
) {
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

    let type_context = program.clone();
    for item in &mut program.items {
        rewrite_item_matching_calls(
            item,
            from_id,
            to_id,
            match_types,
            symbol_types,
            &type_context,
        );
    }
}

fn rewrite_item_matching_calls(
    item: &mut HirItem,
    from: SymbolId,
    to: SymbolId,
    match_types: &[Type],
    symbol_types: &HashMap<SymbolId, Type>,
    program: &HirProgram,
) {
    match item {
        HirItem::Binding(b) => {
            rewrite_expr_matching_calls(&mut b.value, from, to, match_types, symbol_types, program)
        }
        HirItem::Fn(f) => {
            rewrite_block_matching_calls(&mut f.body, from, to, match_types, symbol_types, program)
        }
        HirItem::Watch(w) => {
            rewrite_block_matching_calls(&mut w.body, from, to, match_types, symbol_types, program)
        }
        HirItem::On(o) => {
            rewrite_block_matching_calls(&mut o.body, from, to, match_types, symbol_types, program)
        }
        HirItem::SourceAssign(a) => {
            rewrite_expr_matching_calls(&mut a.value, from, to, match_types, symbol_types, program)
        }
        HirItem::InitIf(i) => {
            rewrite_expr_matching_calls(&mut i.cond, from, to, match_types, symbol_types, program);
            rewrite_block_matching_calls(
                &mut i.then_branch,
                from,
                to,
                match_types,
                symbol_types,
                program,
            );
            if let Some(e) = &mut i.else_branch {
                match e {
                    flux_hir::HirElse::If(inner) => {
                        rewrite_expr_matching_calls(
                            &mut inner.cond,
                            from,
                            to,
                            match_types,
                            symbol_types,
                            program,
                        );
                        rewrite_block_matching_calls(
                            &mut inner.then_branch,
                            from,
                            to,
                            match_types,
                            symbol_types,
                            program,
                        );
                    }
                    flux_hir::HirElse::Block(b) => {
                        rewrite_block_matching_calls(
                            b,
                            from,
                            to,
                            match_types,
                            symbol_types,
                            program,
                        );
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
    symbol_types: &HashMap<SymbolId, Type>,
    program: &HirProgram,
) {
    for stmt in &mut block.stmts {
        match stmt {
            HirStmt::Expr(e) => {
                rewrite_expr_matching_calls(e, from, to, match_types, symbol_types, program)
            }
            HirStmt::Let(s) => rewrite_expr_matching_calls(
                &mut s.value,
                from,
                to,
                match_types,
                symbol_types,
                program,
            ),
            HirStmt::Assign(a) => rewrite_expr_matching_calls(
                &mut a.value,
                from,
                to,
                match_types,
                symbol_types,
                program,
            ),
            HirStmt::Return(r) => {
                if let Some(v) = &mut r.value {
                    rewrite_expr_matching_calls(v, from, to, match_types, symbol_types, program);
                }
            }
            HirStmt::If(i) => {
                rewrite_expr_matching_calls(
                    &mut i.cond,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
                rewrite_block_matching_calls(
                    &mut i.then_branch,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
                if let Some(e) = &mut i.else_branch {
                    match e {
                        flux_hir::HirElse::If(inner) => {
                            rewrite_expr_matching_calls(
                                &mut inner.cond,
                                from,
                                to,
                                match_types,
                                symbol_types,
                                program,
                            );
                            rewrite_block_matching_calls(
                                &mut inner.then_branch,
                                from,
                                to,
                                match_types,
                                symbol_types,
                                program,
                            );
                        }
                        flux_hir::HirElse::Block(b) => {
                            rewrite_block_matching_calls(
                                b,
                                from,
                                to,
                                match_types,
                                symbol_types,
                                program,
                            );
                        }
                    }
                }
            }
            HirStmt::While(w) => {
                rewrite_expr_matching_calls(
                    &mut w.cond,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
                rewrite_block_matching_calls(
                    &mut w.body,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
            }
            HirStmt::For(f) => {
                rewrite_expr_matching_calls(
                    &mut f.iter,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
                rewrite_block_matching_calls(
                    &mut f.body,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
            }
            HirStmt::Watch(w) => rewrite_block_matching_calls(
                &mut w.body,
                from,
                to,
                match_types,
                symbol_types,
                program,
            ),
            HirStmt::On(o) => rewrite_block_matching_calls(
                &mut o.body,
                from,
                to,
                match_types,
                symbol_types,
                program,
            ),
            HirStmt::Emit(e) => {
                for arg in &mut e.args {
                    rewrite_expr_matching_calls(arg, from, to, match_types, symbol_types, program);
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
    symbol_types: &HashMap<SymbolId, Type>,
    program: &HirProgram,
) {
    if let HirExpr::Call { callee, args, .. } = expr {
        if let HirExpr::Ident { symbol, .. } = callee.as_mut() {
            if *symbol == from && call_matches_types(args, match_types, symbol_types, program) {
                *symbol = to;
            }
        }
    }
    match expr {
        HirExpr::Call { callee, args, .. } => {
            rewrite_expr_matching_calls(callee, from, to, match_types, symbol_types, program);
            for a in args {
                rewrite_expr_matching_calls(a, from, to, match_types, symbol_types, program);
            }
        }
        HirExpr::Binary { left, right, .. }
        | HirExpr::Index {
            base: left,
            index: right,
            ..
        } => {
            rewrite_expr_matching_calls(left, from, to, match_types, symbol_types, program);
            rewrite_expr_matching_calls(right, from, to, match_types, symbol_types, program);
        }
        HirExpr::Unary { expr: inner, .. } => {
            rewrite_expr_matching_calls(inner, from, to, match_types, symbol_types, program)
        }
        HirExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            rewrite_expr_matching_calls(cond, from, to, match_types, symbol_types, program);
            rewrite_expr_matching_calls(then_expr, from, to, match_types, symbol_types, program);
            rewrite_expr_matching_calls(else_expr, from, to, match_types, symbol_types, program);
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            rewrite_expr_matching_calls(scrutinee, from, to, match_types, symbol_types, program);
            for arm in arms {
                if let Some(guard) = &mut arm.guard {
                    rewrite_expr_matching_calls(
                        guard,
                        from,
                        to,
                        match_types,
                        symbol_types,
                        program,
                    );
                }
                rewrite_block_matching_calls(
                    &mut arm.body,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
                rewrite_expr_matching_calls(
                    &mut arm.expr,
                    from,
                    to,
                    match_types,
                    symbol_types,
                    program,
                );
            }
        }
        HirExpr::Field { base, .. } => {
            rewrite_expr_matching_calls(base, from, to, match_types, symbol_types, program)
        }
        HirExpr::Snap { expr: inner, .. }
        | HirExpr::Cast { expr: inner, .. }
        | HirExpr::ToText { expr: inner, .. }
        | HirExpr::Try { expr: inner, .. } => {
            rewrite_expr_matching_calls(inner, from, to, match_types, symbol_types, program);
        }
        HirExpr::Array { elems, .. } => {
            for e in elems {
                rewrite_expr_matching_calls(e, from, to, match_types, symbol_types, program);
            }
        }
        HirExpr::StructLit { fields, .. } => {
            for (_, e) in fields {
                rewrite_expr_matching_calls(e, from, to, match_types, symbol_types, program);
            }
        }
        HirExpr::EnumVariant {
            payload: Some(p), ..
        } => {
            rewrite_expr_matching_calls(p, from, to, match_types, symbol_types, program);
        }
        _ => {}
    }
}
