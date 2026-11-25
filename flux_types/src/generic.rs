//! Generic structs and enums: a template's fields mention its parameters as
//! [`Type::Var`]; an instance substitutes its arguments for them. Values carry
//! no type arguments, so only the checker sees them.

use std::collections::HashMap;

use crate::types::Type;

/// The parameter names of a generic template (`Pair[T]` → `["T"]`).
pub(crate) fn template_params(template: &Type) -> Vec<String> {
    match template {
        Type::Struct { args, .. } | Type::Enum { args, .. } => args
            .iter()
            .filter_map(|a| match a {
                Type::Var(name) => Some(name.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `template` with `args` for its parameters. Missing arguments are unknown
/// (`Empty`), to be fixed by context like `None`'s.
pub(crate) fn instantiate(template: &Type, args: &[Type]) -> Type {
    let params = template_params(template);
    if params.is_empty() {
        return template.clone();
    }
    let map: HashMap<&str, Type> = params
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), args.get(i).cloned().unwrap_or(Type::Empty)))
        .collect();
    substitute(template, &map)
}

/// `ty` with each user type as a placeholder that names it with its
/// arguments; `TypeCheckContext::expand` gives the rest when needed.
pub(crate) fn reference(ty: &Type) -> Type {
    match ty {
        Type::Struct { name, args, .. } | Type::Enum { name, args, .. } if !name.is_empty() => {
            Type::Enum {
                name: name.clone(),
                args: args.iter().map(reference).collect(),
                variants: vec![],
            }
        }
        Type::Struct { name, args, fields } => Type::Struct {
            name: name.clone(),
            args: args.clone(),
            fields: fields
                .iter()
                .map(|(n, t)| (n.clone(), reference(t)))
                .collect(),
        },
        Type::Cell(t) => Type::Cell(Box::new(reference(t))),
        Type::Read(t) => Type::Read(Box::new(reference(t))),
        Type::Array(t) => Type::Array(Box::new(reference(t))),
        Type::Option(t) => Type::Option(Box::new(reference(t))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(reference(key)),
            value: Box::new(reference(value)),
        },
        Type::Tuple(elems) => Type::Tuple(elems.iter().map(reference).collect()),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(reference(ok)),
            err: Box::new(reference(err)),
        },
        Type::Func { params, ret } => Type::Func {
            params: params.iter().map(reference).collect(),
            ret: Box::new(reference(ret)),
        },
        other => other.clone(),
    }
}

/// `ty` with each `Var` in `map` replaced. Type arguments hold their types by
/// [`reference`]; fields and payloads hold them in full. A full copy in both
/// would double the type at each level of nesting (`Box[Box[Box[Int]]]`).
pub(crate) fn substitute(ty: &Type, map: &HashMap<&str, Type>) -> Type {
    let sub = |t: &Type| substitute(t, map);
    let arg = |t: &Type| reference(&substitute(t, map));
    match ty {
        Type::Var(name) => map
            .get(name.as_str())
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        Type::Cell(t) => Type::Cell(Box::new(sub(t))),
        Type::Read(t) => Type::Read(Box::new(sub(t))),
        Type::Array(t) => Type::Array(Box::new(sub(t))),
        Type::Option(t) => Type::Option(Box::new(sub(t))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(sub(key)),
            value: Box::new(sub(value)),
        },
        Type::Tuple(elems) => Type::Tuple(elems.iter().map(sub).collect()),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(sub(ok)),
            err: Box::new(sub(err)),
        },
        Type::Event(values) => Type::Event(values.iter().map(sub).collect()),
        Type::Struct { name, args, fields } => Type::Struct {
            name: name.clone(),
            args: args.iter().map(arg).collect(),
            fields: fields.iter().map(|(n, t)| (n.clone(), sub(t))).collect(),
        },
        Type::Enum {
            name,
            args,
            variants,
        } => Type::Enum {
            name: name.clone(),
            args: args.iter().map(arg).collect(),
            variants: variants
                .iter()
                .map(|(n, p)| (n.clone(), p.as_ref().map(sub)))
                .collect(),
        },
        Type::Func { params, ret } => Type::Func {
            params: params.iter().map(sub).collect(),
            ret: Box::new(sub(ret)),
        },
        other => other.clone(),
    }
}

/// Binds the parameters in `template` to the matching parts of `actual`. The
/// first known binding of a parameter stays; a later one that disagrees is
/// found when the instance is checked against the values.
pub(crate) fn bind(template: &Type, actual: &Type, out: &mut HashMap<String, Type>) {
    if actual.is_error() {
        return;
    }
    match (template, actual) {
        (_, Type::Empty) => {}
        (Type::Var(name), _) => {
            let known = out
                .get(name)
                .is_some_and(|t| !crate::check::mentions_empty(t));
            if !known {
                let refined = match out.get(name) {
                    Some(previous) => crate::check::refine_unknown(previous, actual),
                    None => actual.clone(),
                };
                out.insert(name.clone(), refined);
            }
        }
        (Type::Array(t), Type::Array(a))
        | (Type::Option(t), Type::Option(a))
        | (Type::Cell(t), Type::Cell(a))
        | (Type::Read(t), Type::Read(a)) => bind(t, a, out),
        (Type::Map { key: tk, value: tv }, Type::Map { key: ak, value: av }) => {
            bind(tk, ak, out);
            bind(tv, av, out);
        }
        (Type::Result { ok: to, err: te }, Type::Result { ok: ao, err: ae }) => {
            bind(to, ao, out);
            bind(te, ae, out);
        }
        (Type::Tuple(ts), Type::Tuple(as_)) if ts.len() == as_.len() => {
            ts.iter().zip(as_).for_each(|(t, a)| bind(t, a, out));
        }
        (
            Type::Func {
                params: tp,
                ret: tr,
            },
            Type::Func {
                params: ap,
                ret: ar,
            },
        ) if tp.len() == ap.len() => {
            tp.iter().zip(ap).for_each(|(t, a)| bind(t, a, out));
            bind(tr, ar, out);
        }
        (
            Type::Struct {
                name: tn, args: ta, ..
            }
            | Type::Enum {
                name: tn, args: ta, ..
            },
            Type::Struct {
                name: an, args: aa, ..
            }
            | Type::Enum {
                name: an, args: aa, ..
            },
        ) if !tn.is_empty() && tn == an && ta.len() == aa.len() => {
            ta.iter().zip(aa).for_each(|(t, a)| bind(t, a, out));
        }
        // A struct variant's fields.
        (Type::Struct { name, fields, .. }, Type::Struct { fields: af, .. }) if name.is_empty() => {
            for (n, t) in fields {
                if let Some((_, a)) = af.iter().find(|(m, _)| m == n) {
                    bind(t, a, out);
                }
            }
        }
        _ => {}
    }
}

/// The arguments for `template`'s parameters from `bound`; unbound ones are
/// unknown.
pub(crate) fn bound_args(template: &Type, bound: &HashMap<String, Type>) -> Vec<Type> {
    template_params(template)
        .iter()
        .map(|p| bound.get(p).cloned().unwrap_or(Type::Empty))
        .collect()
}

/// The name a generic instance's own methods are found by: its type with
/// canonical arguments (`Pair[Int]` for a `Pair[Int64]`). `None` for a type
/// without type arguments.
pub(crate) fn instance_tag(ty: &Type) -> Option<String> {
    match ty {
        Type::Struct { name, args, .. } | Type::Enum { name, args, .. }
            if !name.is_empty() && !args.is_empty() =>
        {
            let args: Vec<String> = args.iter().map(|a| canonical(a).key()).collect();
            Some(format!("{name}[{}]", args.join(", ")))
        }
        Type::Cell(t) | Type::Read(t) => instance_tag(t),
        _ => None,
    }
}

/// `ty` with `Int64`/`Float64` written `Int`/`Float` and user types as
/// references, so that one type has one key.
fn canonical(ty: &Type) -> Type {
    let c = canonical;
    match ty {
        Type::Int64 => Type::Int,
        Type::Float64 => Type::Float,
        Type::Cell(t) | Type::Read(t) => c(t),
        Type::Array(t) => Type::Array(Box::new(c(t))),
        Type::Option(t) => Type::Option(Box::new(c(t))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(c(key)),
            value: Box::new(c(value)),
        },
        Type::Tuple(elems) => Type::Tuple(elems.iter().map(c).collect()),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(c(ok)),
            err: Box::new(c(err)),
        },
        Type::Func { params, ret } => Type::Func {
            params: params.iter().map(c).collect(),
            ret: Box::new(c(ret)),
        },
        Type::Struct { name, args, .. } | Type::Enum { name, args, .. } if !name.is_empty() => {
            Type::Enum {
                name: name.clone(),
                args: args.iter().map(c).collect(),
                variants: vec![],
            }
        }
        other => other.clone(),
    }
}
