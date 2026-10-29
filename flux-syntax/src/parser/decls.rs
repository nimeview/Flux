//! Parsing of top-level declarations and their parameter lists.

use flux_diagnostics::{SourceId, Span};

use crate::ast::*;

use super::exprs::parse_expr;
use super::stmts::parse_block;
use super::types::{parse_type_ann, parse_type_name};
use super::{parse_ident, span_of, Rule};

fn take_pub(inner: &mut std::iter::Peekable<pest::iterators::Pairs<'_, Rule>>) -> bool {
    if inner
        .peek()
        .is_some_and(|p| p.as_rule() == Rule::pub_modifier)
    {
        inner.next();
        true
    } else {
        false
    }
}

pub(super) fn take_mut(inner: &mut std::iter::Peekable<pest::iterators::Pairs<'_, Rule>>) -> bool {
    if inner
        .peek()
        .is_some_and(|p| p.as_rule() == Rule::mut_modifier)
    {
        inner.next();
        true
    } else {
        false
    }
}

pub(super) fn parse_import(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> ImportDecl {
    let mut inner = pair.into_inner();
    let path = inner.next().expect("import path").as_str().to_string();
    let mut alias = None;
    let mut symbols = Vec::new();
    for child in inner {
        match child.as_rule() {
            Rule::import_as => {
                alias = Some(
                    parse_ident(
                        source_id,
                        child.into_inner().next().expect("import as ident"),
                    )
                    .name,
                );
            }
            Rule::import_symbols => {
                if let Some(list) = child.into_inner().find(|p| p.as_rule() == Rule::ident_list) {
                    symbols = parse_ident_list(source_id, list);
                }
            }
            _ => {}
        }
    }
    ImportDecl {
        path,
        alias,
        symbols,
        span,
    }
}

fn parse_ident_list(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Vec<String> {
    match pair.as_rule() {
        Rule::ident_list => pair
            .into_inner()
            .map(|id| parse_ident(source_id, id).name)
            .collect(),
        Rule::ident => vec![parse_ident(source_id, pair).name],
        _ => pair
            .into_inner()
            .filter(|p| p.as_rule() == Rule::ident)
            .map(|id| parse_ident(source_id, id).name)
            .collect(),
    }
}

pub(super) fn parse_struct(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> StructDecl {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("struct name"));
    let mut type_params = Vec::new();
    let mut fields_pair = None;
    for child in inner {
        match child.as_rule() {
            Rule::type_params => {
                type_params = child
                    .into_inner()
                    .filter(|p| p.as_rule() == Rule::ident)
                    .map(|p| parse_ident(source_id, p).name)
                    .collect();
            }
            Rule::struct_fields => fields_pair = Some(child),
            _ => {}
        }
    }
    let fields_pair = fields_pair.expect("struct fields");
    let fields = fields_pair
        .into_inner()
        .map(|f| {
            let fspan = span_of(source_id, &f);
            let mut fi = f.into_inner();
            let fname = parse_ident(source_id, fi.next().expect("field name"));
            let fty = parse_type_name(source_id, fi.next().expect("field type"));
            StructField {
                name: fname,
                ty: fty,
                span: fspan,
            }
        })
        .collect();
    StructDecl {
        pub_,
        name,
        type_params,
        fields,
        span,
    }
}

pub(super) fn parse_enum(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> EnumDecl {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("enum name"));
    let variants_pair = inner.next().expect("enum variants");
    let variants = variants_pair
        .into_inner()
        .map(|v| {
            let vspan = span_of(source_id, &v);
            let mut vi = v.into_inner();
            let vname = parse_ident(source_id, vi.next().expect("variant name"));
            let payload = vi.next().map(|p| {
                let p = match p.as_rule() {
                    Rule::enum_payload => p.into_inner().next().expect("enum payload inner"),
                    _ => p,
                };
                match p.as_rule() {
                    Rule::tuple_payload => {
                        let ty = parse_type_name(
                            source_id,
                            p.into_inner().next().expect("tuple payload type"),
                        );
                        EnumPayload::Tuple(ty)
                    }
                    Rule::struct_payload => {
                        let fields = p
                            .into_inner()
                            .map(|f| {
                                let mut fi = f.into_inner();
                                let fname = parse_ident(source_id, fi.next().expect("field name"));
                                let fty =
                                    parse_type_name(source_id, fi.next().expect("field type"));
                                (fname, fty)
                            })
                            .collect();
                        EnumPayload::Struct(fields)
                    }
                    _ => unreachable!("enum payload"),
                }
            });
            EnumVariant {
                name: vname,
                payload,
                span: vspan,
            }
        })
        .collect();
    EnumDecl {
        pub_,
        name,
        variants,
        span,
    }
}

pub(super) fn parse_event(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> EventDecl {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("event name"));
    let params = inner
        .next()
        .map(|p| parse_params_from(source_id, p))
        .unwrap_or_default();
    EventDecl {
        pub_,
        name,
        params,
        span,
    }
}

pub(super) fn parse_fn(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> FnDecl {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("fn name"));
    let mut type_params = Vec::new();
    let mut sig = None;
    let mut body = None;
    for child in inner {
        match child.as_rule() {
            Rule::type_params => {
                type_params = child
                    .into_inner()
                    .filter(|p| p.as_rule() == Rule::ident)
                    .map(|p| parse_ident(source_id, p).name)
                    .collect();
            }
            Rule::fn_sig => sig = Some(child),
            Rule::block => body = Some(parse_block(source_id, child)),
            _ => {}
        }
    }
    let (params, ret) = parse_fn_sig(source_id, sig.expect("fn sig"));
    FnDecl {
        pub_,
        name,
        type_params,
        params,
        ret,
        body: body.expect("fn body"),
        span,
    }
}

pub(super) fn parse_rfn(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> RFnDecl {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("rfn name"));
    let mut type_params = Vec::new();
    let mut sig = None;
    let mut body = None;
    for child in inner {
        match child.as_rule() {
            Rule::type_params => {
                type_params = child
                    .into_inner()
                    .filter(|p| p.as_rule() == Rule::ident)
                    .map(|p| parse_ident(source_id, p).name)
                    .collect();
            }
            Rule::fn_sig => sig = Some(child),
            Rule::block => body = Some(parse_block(source_id, child)),
            _ => {}
        }
    }
    let (params, ret) = parse_fn_sig(source_id, sig.expect("rfn sig"));
    RFnDecl {
        pub_,
        name,
        type_params,
        params,
        ret,
        body: body.expect("rfn body"),
        span,
    }
}

pub(super) fn parse_trait(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> TraitDecl {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("trait name"));
    let body = inner.next().expect("trait body");
    let methods = body
        .into_inner()
        .map(|m| {
            let mspan = span_of(source_id, &m);
            let mut mname = None;
            let mut sig = None;
            for child in m.into_inner() {
                match child.as_rule() {
                    Rule::ident => mname = Some(parse_ident(source_id, child)),
                    Rule::fn_sig => sig = Some(child),
                    _ => {}
                }
            }
            let mname = mname.expect("trait method name");
            let (params, ret) = parse_fn_sig(source_id, sig.expect("trait method sig"));
            TraitMethod {
                name: mname,
                params,
                ret,
                span: mspan,
            }
        })
        .collect();
    TraitDecl {
        pub_,
        name,
        methods,
        span,
    }
}

pub(super) fn parse_impl(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> ImplDecl {
    let mut inner = pair.into_inner();
    let trait_name = parse_ident(source_id, inner.next().expect("trait name"));
    let target_type = parse_type_name(source_id, inner.next().expect("impl type"));
    let body = inner.next().expect("impl body");
    let methods = body
        .into_inner()
        .map(|m| {
            let mspan = span_of(source_id, &m);
            let mut mname = None;
            let mut sig = None;
            let mut block = None;
            for child in m.into_inner() {
                match child.as_rule() {
                    Rule::ident => mname = Some(parse_ident(source_id, child)),
                    Rule::fn_sig => sig = Some(child),
                    Rule::block => block = Some(parse_block(source_id, child)),
                    _ => {}
                }
            }
            let mname = mname.expect("impl method name");
            let (params, ret) = parse_fn_sig(source_id, sig.expect("impl method sig"));
            FnDecl {
                pub_: false,
                name: mname,
                type_params: vec![],
                params,
                ret,
                body: block.expect("impl method body"),
                span: mspan,
            }
        })
        .collect();
    ImplDecl {
        trait_name,
        target_type,
        methods,
        span,
    }
}

fn parse_fn_sig(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> (Vec<Param>, Option<TypeName>) {
    let mut params = Vec::new();
    let mut ret = None;
    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::param_list => params = parse_param_list(source_id, child),
            Rule::return_type => {
                let ty = child.into_inner().next().expect("return type");
                ret = Some(parse_type_name(source_id, ty));
            }
            _ => {}
        }
    }
    (params, ret)
}

fn parse_params_from(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Vec<Param> {
    match pair.as_rule() {
        Rule::param_list => parse_param_list(source_id, pair),
        Rule::event_params | Rule::fn_sig => pair
            .into_inner()
            .find(|p| p.as_rule() == Rule::param_list)
            .map(|p| parse_param_list(source_id, p))
            .unwrap_or_default(),
        Rule::param => vec![parse_param(source_id, pair)],
        _ => vec![],
    }
}

fn parse_param(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Param {
    let span = span_of(source_id, &pair);
    let mut inner = pair.into_inner();
    let name = parse_ident(source_id, inner.next().expect("param name"));
    let ty = parse_type_name(source_id, inner.next().expect("param type"));
    Param { name, ty, span }
}

fn parse_param_list(source_id: SourceId, pair: pest::iterators::Pair<'_, Rule>) -> Vec<Param> {
    pair.into_inner()
        .map(|p| parse_param(source_id, p))
        .collect()
}

pub(super) fn parse_closure_param(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> ClosureParam {
    let span = span_of(source_id, &pair);
    let mut inner = pair.into_inner();
    let name = parse_ident(source_id, inner.next().expect("closure param name"));
    let ty = inner.next().map(|t| parse_type_name(source_id, t));
    ClosureParam { name, ty, span }
}

pub(super) fn parse_type_alias(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> TypeAliasDecl {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("type alias name"));
    let ty = parse_type_name(source_id, inner.next().expect("type alias type"));
    TypeAliasDecl {
        pub_,
        name,
        ty,
        span,
    }
}

pub(super) fn parse_const_binding(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> ConstBinding {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let name = parse_ident(source_id, inner.next().expect("const name"));
    let mut ty = None;
    let mut value = None;
    for child in inner {
        match child.as_rule() {
            Rule::type_ann => ty = Some(parse_type_ann(source_id, child)),
            Rule::expr => value = Some(parse_expr(source_id, child)),
            _ => {}
        }
    }
    ConstBinding {
        pub_,
        name,
        ty,
        value: value.expect("const value"),
        span,
    }
}

pub(super) fn parse_ident_list_fields(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> Vec<Ident> {
    match pair.as_rule() {
        Rule::ident_list => pair
            .into_inner()
            .map(|p| parse_ident(source_id, p))
            .collect(),
        Rule::ident => vec![parse_ident(source_id, pair)],
        _ => pair
            .into_inner()
            .filter(|p| p.as_rule() == Rule::ident)
            .map(|p| parse_ident(source_id, p))
            .collect(),
    }
}

pub(super) fn parse_let_pattern(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
) -> LetPattern {
    let span = span_of(source_id, &pair);
    match pair.as_rule() {
        Rule::let_pattern => parse_let_pattern(
            source_id,
            pair.into_inner().next().expect("let pattern inner"),
        ),
        Rule::ident => LetPattern::Ident(parse_ident(source_id, pair)),
        Rule::struct_pat => {
            let mut inner = pair.into_inner();
            let name = parse_ident(source_id, inner.next().expect("struct pat name"));
            let fields = inner
                .next()
                .map(|list| parse_ident_list_fields(source_id, list))
                .unwrap_or_default();
            LetPattern::Struct { name, fields, span }
        }
        other => panic!("unexpected let pattern rule: {other:?}"),
    }
}

pub(super) fn parse_let_binding(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> LetStmt {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let mutable = take_mut(&mut inner);
    let pattern = parse_let_pattern(source_id, inner.next().expect("let pattern"));
    let mut ty = None;
    let mut value = None;
    for child in inner {
        match child.as_rule() {
            Rule::type_ann => ty = Some(parse_type_ann(source_id, child)),
            Rule::expr => value = Some(parse_expr(source_id, child)),
            _ => {}
        }
    }
    LetStmt {
        pub_,
        mutable,
        pattern,
        ty,
        value: value.expect("let value"),
        span,
    }
}

pub(super) fn parse_derived_binding(
    source_id: SourceId,
    pair: pest::iterators::Pair<'_, Rule>,
    span: Span,
) -> DerivedStmt {
    let mut inner = pair.into_inner().peekable();
    let pub_ = take_pub(&mut inner);
    let pattern = parse_let_pattern(source_id, inner.next().expect("derived pattern"));
    let mut ty = None;
    let mut value = None;
    for child in inner {
        match child.as_rule() {
            Rule::type_ann => ty = Some(parse_type_ann(source_id, child)),
            Rule::expr => value = Some(parse_expr(source_id, child)),
            _ => {}
        }
    }
    DerivedStmt {
        pub_,
        pattern,
        ty,
        value: value.expect("derived value"),
        span,
    }
}
