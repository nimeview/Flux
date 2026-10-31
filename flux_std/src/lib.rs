//! Flux standard library — builtin names and native implementations.

mod io;
mod math;
mod modules;
mod str_ext;

pub use modules::{
    export_in_std_module, is_std_module, std_module_exports, std_module_name, STD_MODULES,
};

pub use flux_value::Value;

pub const BUILTIN_FNS: &[&str] = &[
    "print",
    "println",
    "eprint",
    "eprintln",
    "args",
    "len",
    "range",
    "push",
    "pop",
    "get",
    "map",
    "filter",
    "reduce",
    "str_len",
    "str_split",
    "str_contains",
    "str_trim",
    "str_join",
    "str_starts_with",
    "str_ends_with",
    "str_replace",
    "str_to_lower",
    "str_to_upper",
    "to_string",
    "parse_int",
    "parse_float",
    "abs",
    "min",
    "max",
    "clamp",
    "sqrt",
    "floor",
    "ceil",
    "pow",
    "assert",
    "assert_eq",
    "read_line",
    "read_file",
    "write_file",
    "append_file",
    "file_exists",
    "Some",
    "None",
    "Ok",
    "Err",
];

/// Pure intrinsics eligible for JIT libcalls (no I/O, no HOF).
pub const PURE_INTRINSICS: &[&str] = &[
    "len",
    "range",
    "abs",
    "min",
    "max",
    "clamp",
    "sqrt",
    "floor",
    "ceil",
    "pow",
    "str_len",
    "str_contains",
    "str_starts_with",
    "str_ends_with",
    "str_to_lower",
    "str_to_upper",
];

/// Builtin calls with side effects (I/O, printing) — forbidden in derived expressions.
pub const SIDE_EFFECT_BUILTINS: &[&str] = &[
    "print",
    "println",
    "eprint",
    "eprintln",
    "args",
    "read_line",
    "read_file",
    "write_file",
    "append_file",
];

pub const HOF_FNS: &[&str] = &["map", "filter", "reduce"];

pub fn is_hof(name: &str) -> bool {
    HOF_FNS.contains(&name)
}

/// Returns `true` if `name` is a builtin function.
pub fn is_builtin_fn(name: &str) -> bool {
    BUILTIN_FNS.contains(&name)
}

/// Call a builtin function. Returns `None` if `name` is not a builtin.
/// Like [`call_builtin`], but reports runtime errors (e.g. a `range` longer than
/// [`flux_value::MAX_COLLECTION_LEN`]) instead of returning a placeholder value.
pub fn call_builtin_checked(
    name: &str,
    args: &[Value],
) -> Option<Result<Value, flux_value::ArithError>> {
    if name == "range" {
        let (start, end, step) = range_args(args)?;
        return Some(
            flux_value::int_range(start, end, step)
                .map(|items| Value::Array(items.into_iter().map(Value::int_i64).collect())),
        );
    }
    call_builtin(name, args).map(Ok)
}

/// `range(end)`, `range(start, end)`, or `range(start, end, step)`.
fn range_args(args: &[Value]) -> Option<(i64, i64, i64)> {
    Some(match args.len() {
        0 => (0, 0, 1),
        1 => (0, int_arg(args.first())?, 1),
        2 => (int_arg(args.first())?, int_arg(args.get(1))?, 1),
        _ => (
            int_arg(args.first())?,
            int_arg(args.get(1))?,
            int_arg(args.get(2))?,
        ),
    })
}

static PROGRAM_ARGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Sets what `args()` returns: the arguments after `--` in
/// `flux run file.fl -- a b`. Only the first call takes effect.
pub fn set_program_args(args: Vec<String>) {
    let _ = PROGRAM_ARGS.set(args);
}

fn program_args() -> Vec<String> {
    PROGRAM_ARGS.get().cloned().unwrap_or_default()
}

/// What `print`/`println` write: a top-level string as its raw text; any other
/// value (including strings nested in arrays) in its quoted display form.
fn print_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.display_string(),
    }
}

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "print" => {
            if let Some(v) = args.first() {
                print!("{}", print_text(v));
            }
            Some(Value::Unit)
        }
        "println" => {
            if let Some(v) = args.first() {
                println!("{}", print_text(v));
            } else {
                println!();
            }
            Some(Value::Unit)
        }
        "eprint" => {
            if let Some(v) = args.first() {
                eprint!("{}", print_text(v));
            }
            Some(Value::Unit)
        }
        "eprintln" => {
            if let Some(v) = args.first() {
                eprintln!("{}", print_text(v));
            } else {
                eprintln!();
            }
            Some(Value::Unit)
        }
        "args" => Some(Value::Array(
            program_args().into_iter().map(Value::String).collect(),
        )),
        "len" => {
            let n = match args.first()? {
                Value::String(s) => s.chars().count() as i64,
                Value::Array(a) => a.len() as i64,
                _ => return Some(Value::int_i64(0)),
            };
            Some(Value::int_i64(n))
        }
        // Over-long ranges are an error only through `call_builtin_checked`.
        "range" => call_builtin_checked(name, args)?.ok(),
        "push" => {
            let arr = match args.first()? {
                Value::Array(a) => a.clone(),
                _ => vec![],
            };
            let val = args.get(1).cloned().unwrap_or(Value::Unit);
            let mut out = arr;
            out.push(val);
            Some(Value::Array(out))
        }
        // `get(xs, i)`: `Some(xs[i])` when `0 <= i < len(xs)`, otherwise `None`.
        "get" => {
            let item = match (args.first()?, int_arg(args.get(1))) {
                (Value::Array(items), Some(i)) => {
                    usize::try_from(i).ok().and_then(|i| items.get(i)).cloned()
                }
                _ => None,
            };
            Some(Value::Option(item.map(Box::new)))
        }
        "pop" => {
            let mut arr = match args.first()? {
                Value::Array(a) => a.clone(),
                _ => vec![],
            };
            let last = arr.pop().unwrap_or(Value::Unit);
            Some(Value::Array(vec![Value::Array(arr), last]))
        }
        // `map`/`filter`/`reduce` are higher-order and need to invoke user functions
        // or closures, which this value-only builtin layer can't do. The VM
        // (`flux-runtime/src/exec.rs::call_function`) intercepts these names before
        // ever reaching `call_builtin`, so they're intentionally unhandled here.
        "str_len" => {
            let n = match args.first()? {
                Value::String(s) => s.chars().count() as i64,
                _ => 0,
            };
            Some(Value::int_i64(n))
        }
        "str_split" => {
            let s = match args.first()? {
                Value::String(s) => s,
                _ => return Some(Value::Array(vec![])),
            };
            let sep = match args.get(1)? {
                Value::String(s) => s.as_str(),
                _ => " ",
            };
            let parts: Vec<Value> = s.split(sep).map(|p| Value::String(p.to_string())).collect();
            Some(Value::Array(parts))
        }
        "str_contains" => {
            let (hay, needle) = match (args.first(), args.get(1)) {
                (Some(Value::String(h)), Some(Value::String(n))) => (h, n),
                _ => return Some(Value::Bool(false)),
            };
            Some(Value::Bool(hay.contains(needle.as_str())))
        }
        "str_trim" => {
            let s = match args.first()? {
                Value::String(s) => s.trim().to_string(),
                _ => String::new(),
            };
            Some(Value::String(s))
        }
        "str_join" => {
            let arr = match args.first()? {
                Value::Array(a) => a,
                _ => return Some(Value::String(String::new())),
            };
            let sep = match args.get(1) {
                Some(Value::String(s)) => s.as_str(),
                _ => "",
            };
            let parts: Vec<String> = arr
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.display_string(),
                })
                .collect();
            Some(Value::String(parts.join(sep)))
        }
        "str_starts_with" => Some(str_ext::starts_with(args.first()?, args.get(1)?)?),
        "str_ends_with" => Some(str_ext::ends_with(args.first()?, args.get(1)?)?),
        "str_replace" => Some(str_ext::replace(args.first()?, args.get(1)?, args.get(2)?)?),
        "str_to_lower" => Some(str_ext::to_lower(args.first()?)?),
        "str_to_upper" => Some(str_ext::to_upper(args.first()?)?),
        "to_string" => Some(Value::String(print_text(args.first()?))),
        "parse_int" => Some(str_ext::parse_int(args.first()?)),
        "parse_float" => Some(str_ext::parse_float(args.first()?)),
        "abs" => math::abs(args.first()?),
        "min" => math::min(args.first()?, args.get(1)?),
        "max" => math::max(args.first()?, args.get(1)?),
        "clamp" => math::clamp(args.first()?, args.get(1)?, args.get(2)?),
        "sqrt" => math::sqrt(args.first()?),
        "floor" => math::floor(args.first()?),
        "ceil" => math::ceil(args.first()?),
        "pow" => math::pow(args.first()?, args.get(1)?),
        "assert" => math::assert(args.first()?),
        "assert_eq" => math::assert_eq(args.first()?, args.get(1)?),
        "read_line" => Some(io::read_line()),
        "read_file" => {
            let path = match args.first()? {
                Value::String(s) => s.as_str(),
                _ => return None,
            };
            Some(io::read_file(path))
        }
        "write_file" => {
            let path = match args.first()? {
                Value::String(s) => s.as_str(),
                _ => return None,
            };
            let contents = match args.get(1)? {
                Value::String(s) => s.as_str(),
                _ => return None,
            };
            Some(io::write_file(path, contents))
        }
        "append_file" => {
            let path = match args.first()? {
                Value::String(s) => s.as_str(),
                _ => return None,
            };
            let contents = match args.get(1)? {
                Value::String(s) => s.as_str(),
                _ => return None,
            };
            Some(io::append_file(path, contents))
        }
        "file_exists" => {
            let path = match args.first()? {
                Value::String(s) => s.as_str(),
                _ => return Some(Value::Bool(false)),
            };
            Some(io::file_exists(path))
        }
        "Some" => {
            let inner = args.first().cloned().unwrap_or(Value::Unit);
            Some(Value::Option(Some(Box::new(inner))))
        }
        "None" => Some(Value::Option(None)),
        "Ok" => {
            let inner = args.first().cloned().unwrap_or(Value::Unit);
            Some(Value::Result(Ok(Box::new(inner))))
        }
        "Err" => {
            let inner = args
                .first()
                .cloned()
                .unwrap_or(Value::String(String::new()));
            Some(Value::Result(Err(Box::new(inner))))
        }
        _ => None,
    }
}

fn int_arg(v: Option<&Value>) -> Option<i64> {
    v?.as_int()
}
