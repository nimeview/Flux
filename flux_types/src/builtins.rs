use crate::Type;

/// All builtin function names.
pub const BUILTIN_FNS: &[&str] = flux_std::BUILTIN_FNS;

/// Type signature of a builtin function, if known.
pub fn builtin_fn_type(name: &str) -> Option<Type> {
    match name {
        "print" | "println" | "eprint" | "eprintln" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::Unit),
        }),
        "args" => Some(Type::Func {
            params: vec![],
            ret: Box::new(Type::Array(Box::new(Type::String))),
        }),
        "len" => Some(Type::Func {
            params: vec![Type::Array(Box::new(Type::Int))],
            ret: Box::new(Type::Int),
        }),
        "range" => Some(Type::Func {
            params: vec![Type::Int],
            ret: Box::new(Type::Array(Box::new(Type::Int))),
        }),
        "push" => Some(Type::Func {
            params: vec![Type::Array(Box::new(Type::Int)), Type::Int],
            ret: Box::new(Type::Array(Box::new(Type::Int))),
        }),
        "get" => Some(Type::Func {
            params: vec![Type::Array(Box::new(Type::Var("T".into()))), Type::Int],
            ret: Box::new(Type::Option(Box::new(Type::Var("T".into())))),
        }),
        "pop" => Some(Type::Func {
            params: vec![Type::Array(Box::new(Type::Int))],
            ret: Box::new(Type::Array(Box::new(Type::Int))),
        }),
        // Signatures below are illustrative placeholders using `Type::Var` for the
        // element type `T` / accumulator `U`. Actual call sites are type-checked
        // parametrically in `check::infer_builtin_call`, which infers `T`/`U` from
        // the argument expressions rather than relying on this fixed signature.
        "map" => Some(Type::Func {
            params: vec![
                Type::Array(Box::new(Type::Var("T".into()))),
                Type::Func {
                    params: vec![Type::Var("T".into())],
                    ret: Box::new(Type::Var("U".into())),
                },
            ],
            ret: Box::new(Type::Array(Box::new(Type::Var("U".into())))),
        }),
        "filter" => Some(Type::Func {
            params: vec![
                Type::Array(Box::new(Type::Var("T".into()))),
                Type::Func {
                    params: vec![Type::Var("T".into())],
                    ret: Box::new(Type::Bool),
                },
            ],
            ret: Box::new(Type::Array(Box::new(Type::Var("T".into())))),
        }),
        "reduce" => Some(Type::Func {
            params: vec![
                Type::Array(Box::new(Type::Var("T".into()))),
                Type::Var("U".into()),
                Type::Func {
                    params: vec![Type::Var("U".into()), Type::Var("T".into())],
                    ret: Box::new(Type::Var("U".into())),
                },
            ],
            ret: Box::new(Type::Var("U".into())),
        }),
        "str_join" => Some(Type::Func {
            params: vec![Type::Array(Box::new(Type::String)), Type::String],
            ret: Box::new(Type::String),
        }),
        "str_len" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::Int),
        }),
        "str_split" => Some(Type::Func {
            params: vec![Type::String, Type::String],
            ret: Box::new(Type::Array(Box::new(Type::String))),
        }),
        "str_contains" => Some(Type::Func {
            params: vec![Type::String, Type::String],
            ret: Box::new(Type::Bool),
        }),
        "str_trim" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::String),
        }),
        "to_string" => Some(Type::Func {
            params: vec![Type::Var("T".into())],
            ret: Box::new(Type::String),
        }),
        "parse_int" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::Result {
                ok: Box::new(Type::Int64),
                err: Box::new(Type::String),
            }),
        }),
        "parse_float" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::Result {
                ok: Box::new(Type::Float64),
                err: Box::new(Type::String),
            }),
        }),
        "read_line" => Some(Type::Func {
            params: vec![],
            ret: Box::new(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::String),
            }),
        }),
        "read_file" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::String),
            }),
        }),
        "write_file" | "append_file" => Some(Type::Func {
            params: vec![Type::String, Type::String],
            ret: Box::new(Type::Result {
                ok: Box::new(Type::Unit),
                err: Box::new(Type::String),
            }),
        }),
        "file_exists" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::Bool),
        }),
        "Some" => Some(Type::Func {
            params: vec![Type::Int],
            ret: Box::new(Type::Option(Box::new(Type::Int))),
        }),
        "None" => Some(Type::Func {
            params: vec![],
            ret: Box::new(Type::Option(Box::new(Type::Unit))),
        }),
        "Ok" => Some(Type::Func {
            params: vec![Type::Int],
            ret: Box::new(Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::String),
            }),
        }),
        "Err" => Some(Type::Func {
            params: vec![Type::String],
            ret: Box::new(Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::String),
            }),
        }),
        _ => None,
    }
}

pub fn is_builtin_fn(name: &str) -> bool {
    flux_std::is_builtin_fn(name)
}
