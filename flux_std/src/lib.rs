//! Flux standard library — builtin names and native implementations.

mod io;
mod math;
mod modules;
mod str_ext;

pub use modules::{
    export_in_std_module, is_std_module, std_module_exports, std_module_name, STD_MODULES,
};

use flux_value::MapKey;
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
    "set",
    "map_new",
    "map_has",
    "map_remove",
    "map_keys",
    "map_values",
    "pop",
    "get",
    "map",
    "filter",
    "reduce",
    "str_len",
    "str_split",
    "str_chars",
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
    "to_string",
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
    match name {
        "assert" => return Some(math::assert(args.first()?)),
        "assert_eq" => return Some(math::assert_eq(args.first()?, args.get(1)?)),
        // Integer operands promote like arithmetic does; a value that does not
        // fit the promoted width (`min(-1, 0u64)`) is the same overflow error
        // as `-1 + 0u64`, not a missing result.
        "min" | "max" | "clamp" if args.iter().all(|a| matches!(a, Value::Int { .. })) => {
            return Some(call_builtin(name, args).ok_or(flux_value::ArithError::Overflow));
        }
        _ => {}
    }
    // `set(m, k, v)` on a map inserts or replaces `k`.
    if name == "set" {
        if let (Some(Value::Map(entries)), Some(k), Some(v)) =
            (args.first(), args.get(1), args.get(2))
        {
            let key = MapKey::of(k)?;
            if !entries.contains_key(&key) && entries.len() as u64 >= flux_value::MAX_COLLECTION_LEN
            {
                return Some(Err(flux_value::ArithError::AllocationLimit));
            }
            let mut entries = (**entries).clone();
            entries.insert(key, (k.clone(), v.clone()));
            return Some(Ok(Value::Map(std::sync::Arc::new(entries))));
        }
    }
    // `set(xs, i, v)`: `xs` with element `i` replaced; `i` must be in range.
    if name == "set" {
        let (Some(Value::Array(items)), Some(i), Some(v)) =
            (args.first(), int_arg(args.get(1)), args.get(2))
        else {
            return None;
        };
        let Some(i) = usize::try_from(i).ok().filter(|&i| i < items.len()) else {
            return Some(Err(flux_value::ArithError::IndexOutOfBounds));
        };
        let mut items = items.to_vec();
        items[i] = v.clone();
        return Some(Ok(Value::array(items)));
    }
    // Arrays stay within the collection limit, as `range` does.
    if name == "push" {
        if let Some(Value::Array(items)) = args.first() {
            if items.len() as u64 >= flux_value::MAX_COLLECTION_LEN {
                return Some(Err(flux_value::ArithError::AllocationLimit));
            }
        }
    }
    if name == "range" {
        let (start, end, step) = range_args(args)?;
        return Some(
            flux_value::int_range(start, end, step)
                .map(|items| Value::array(items.into_iter().map(Value::int_i64).collect())),
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

thread_local! {
    static FAILURE_DETAIL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Records extra text for the next runtime error on this thread (e.g. the
/// values an `assert_eq` compared); the runtime takes it when it raises.
pub fn set_failure_detail(detail: String) {
    FAILURE_DETAIL.with(|d| *d.borrow_mut() = Some(detail));
}

pub fn take_failure_detail() -> Option<String> {
    FAILURE_DETAIL.with(|d| d.borrow_mut().take())
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
        "print" | "println" | "eprint" | "eprintln" => {
            let mut text = args.first().map(print_text).unwrap_or_default();
            if name.ends_with("ln") {
                text.push('\n');
            }
            // `print!` panics when the reader has gone away (`flux run app.fl |
            // head`); a closed stream just drops the output instead.
            use std::io::Write;
            let _ = if name.starts_with('e') {
                std::io::stderr().write_all(text.as_bytes())
            } else {
                std::io::stdout().write_all(text.as_bytes())
            };
            Some(Value::Unit)
        }
        "args" => Some(Value::array(
            program_args().into_iter().map(Value::String).collect(),
        )),
        "len" => {
            let n = match args.first()? {
                Value::String(s) => s.chars().count() as i64,
                Value::Array(a) => a.len() as i64,
                Value::Map(m) => m.len() as i64,
                _ => return Some(Value::int_i64(0)),
            };
            Some(Value::int_i64(n))
        }
        // Over-long ranges are an error only through `call_builtin_checked`.
        "range" => call_builtin_checked(name, args)?.ok(),
        "push" => {
            let arr = match args.first()? {
                Value::Array(a) => a.to_vec(),
                _ => vec![],
            };
            let val = args.get(1).cloned().unwrap_or(Value::Unit);
            let mut out = arr;
            out.push(val);
            Some(Value::array(out))
        }
        // `get(xs, i)`: `Some(xs[i])` when `0 <= i < len(xs)`, otherwise `None`.
        "get" => {
            let item = match (args.first()?, args.get(1)) {
                (Value::Map(entries), Some(k)) => MapKey::of(k)
                    .and_then(|key| entries.get(&key))
                    .map(|(_, v)| v.clone()),
                (Value::Array(items), k) => int_arg(k)
                    .and_then(|i| usize::try_from(i).ok())
                    .and_then(|i| items.get(i))
                    .cloned(),
                _ => None,
            };
            Some(Value::Option(item.map(Box::new)))
        }
        // `Map[K, V]` operations. Keys are ordered (integers by value, strings
        // by code point), so `map_keys`/`map_values` are deterministic.
        "map_new" => Some(Value::Map(Default::default())),
        "map_has" => match (args.first()?, args.get(1)?) {
            (Value::Map(entries), k) => Some(Value::Bool(
                MapKey::of(k).is_some_and(|key| entries.contains_key(&key)),
            )),
            _ => Some(Value::Bool(false)),
        },
        "map_remove" => match (args.first()?, args.get(1)?) {
            (Value::Map(entries), k) => {
                let mut entries = (**entries).clone();
                if let Some(key) = MapKey::of(k) {
                    entries.remove(&key);
                }
                Some(Value::Map(std::sync::Arc::new(entries)))
            }
            _ => None,
        },
        "map_keys" => match args.first()? {
            Value::Map(entries) => Some(Value::array(
                entries.values().map(|(k, _)| k.clone()).collect(),
            )),
            _ => Some(Value::array(vec![])),
        },
        "map_values" => match args.first()? {
            Value::Map(entries) => Some(Value::array(
                entries.values().map(|(_, v)| v.clone()).collect(),
            )),
            _ => Some(Value::array(vec![])),
        },
        "pop" => {
            let mut arr = match args.first()? {
                Value::Array(a) => a.to_vec(),
                _ => vec![],
            };
            // `fn([T]) -> [T]` (SPEC): the array without its last element.
            arr.pop();
            Some(Value::array(arr))
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
        // One string per character (Unicode scalar value); `str_split(s, "")`
        // also yields empty strings at both ends.
        "str_chars" => match args.first()? {
            Value::String(s) => Some(Value::array(
                s.chars().map(|c| Value::String(c.to_string())).collect(),
            )),
            _ => Some(Value::array(vec![])),
        },
        "str_split" => {
            let s = match args.first()? {
                Value::String(s) => s,
                _ => return Some(Value::array(vec![])),
            };
            let sep = match args.get(1)? {
                Value::String(s) => s.as_str(),
                _ => " ",
            };
            let parts: Vec<Value> = s.split(sep).map(|p| Value::String(p.to_string())).collect();
            Some(Value::array(parts))
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
        // Assertion failures are errors; only `call_builtin_checked` reports them.
        "assert" => math::assert(args.first()?).ok(),
        "assert_eq" => math::assert_eq(args.first()?, args.get(1)?).ok(),
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
