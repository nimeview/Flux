//! Library functions reached only through method syntax (`xs.sorted()`,
//! `o.unwrap_or(0)`, `s.split_once(",")`). Their names start with a NUL so
//! that no program can name, shadow, or import them; the methods that take a
//! function (`find`, `any`, `sorted_by`, `map` on `Option`, …) run in the VM,
//! which can call closures (see [`METHOD_HOFS`]).

use std::cmp::Ordering;

use flux_value::{ArithError, MapKey, Value};

/// Every method-only library function.
pub const METHOD_FNS: &[&str] = &[
    "\0contains",
    "\0enumerate",
    "\0zip",
    "\0slice",
    "\0sorted",
    "\0reversed",
    "\0first",
    "\0last",
    "\0concat",
    "\0unwrap_or",
    "\0ok_or",
    "\0is_some",
    "\0is_none",
    "\0is_ok",
    "\0is_err",
    "\0get_or",
    "\0entries",
    "\0split_once",
    "\0lines",
    "\0words",
    "\0find",
    "\0any",
    "\0all",
    "\0sorted_by",
    "\0map_value",
    "\0and_then",
    "\0map_err",
];

/// The ones that call a function argument; the VM runs them.
pub const METHOD_HOFS: &[&str] = &[
    "\0find",
    "\0any",
    "\0all",
    "\0sorted_by",
    "\0map_value",
    "\0and_then",
    "\0map_err",
];

/// The order `sorted` uses: numbers by value, strings by code point.
pub fn order(a: &Value, b: &Value) -> Ordering {
    flux_value::compare(a, b).unwrap_or(Ordering::Equal)
}

fn items(v: Option<&Value>) -> Option<&std::sync::Arc<Vec<Value>>> {
    match v? {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

fn int(v: Option<&Value>) -> Option<i64> {
    v?.as_int()
}

/// Runs a method-only library function that takes no function argument.
pub fn call(name: &str, args: &[Value]) -> Option<Result<Value, ArithError>> {
    let ok = |v: Value| Some(Ok(v));
    match name {
        "\0contains" => {
            let xs = items(args.first())?;
            let needle = args.get(1)?;
            ok(Value::Bool(
                xs.iter().any(|x| flux_value::values_equal(x, needle)),
            ))
        }
        "\0enumerate" => {
            let xs = items(args.first())?;
            ok(Value::array(
                xs.iter()
                    .enumerate()
                    .map(|(i, x)| Value::tuple(vec![Value::int_i64(i as i64), x.clone()]))
                    .collect(),
            ))
        }
        "\0zip" => {
            let (a, b) = (items(args.first())?, items(args.get(1))?);
            ok(Value::array(
                a.iter()
                    .zip(b.iter())
                    .map(|(x, y)| Value::tuple(vec![x.clone(), y.clone()]))
                    .collect(),
            ))
        }
        // `xs.slice(a, b)`: elements `a` up to, not including, `b`.
        "\0slice" => {
            let xs = items(args.first())?;
            // An index past `i64` (a large `UInt`) is out of bounds too.
            let (Some(a), Some(b)) = (int(args.get(1)), int(args.get(2))) else {
                return Some(Err(ArithError::IndexOutOfBounds));
            };
            let fits = 0 <= a && a <= b && (b as u64) <= xs.len() as u64;
            if !fits {
                return Some(Err(ArithError::IndexOutOfBounds));
            }
            ok(Value::array(xs[a as usize..b as usize].to_vec()))
        }
        "\0sorted" => {
            let mut xs = items(args.first())?.to_vec();
            xs.sort_by(order);
            ok(Value::array(xs))
        }
        "\0reversed" => {
            let mut xs = items(args.first())?.to_vec();
            xs.reverse();
            ok(Value::array(xs))
        }
        "\0first" => ok(Value::Option(
            items(args.first())?.first().cloned().map(Box::new),
        )),
        "\0last" => ok(Value::Option(
            items(args.first())?.last().cloned().map(Box::new),
        )),
        "\0concat" => {
            let (a, b) = (items(args.first())?, items(args.get(1))?);
            if (a.len() + b.len()) as u64 > flux_value::MAX_COLLECTION_LEN {
                return Some(Err(ArithError::AllocationLimit));
            }
            let mut out = a.to_vec();
            out.extend(b.iter().cloned());
            ok(Value::array(out))
        }
        "\0unwrap_or" => match args.first()? {
            Value::Option(Some(v)) | Value::Result(Ok(v)) => ok((**v).clone()),
            _ => ok(args.get(1)?.clone()),
        },
        "\0ok_or" => match args.first()? {
            Value::Option(Some(v)) => ok(Value::Result(Ok(v.clone()))),
            _ => ok(Value::Result(Err(Box::new(args.get(1)?.clone())))),
        },
        "\0is_some" => ok(Value::Bool(matches!(args.first()?, Value::Option(Some(_))))),
        "\0is_none" => ok(Value::Bool(matches!(args.first()?, Value::Option(None)))),
        "\0is_ok" => ok(Value::Bool(matches!(args.first()?, Value::Result(Ok(_))))),
        "\0is_err" => ok(Value::Bool(matches!(args.first()?, Value::Result(Err(_))))),
        "\0get_or" => {
            let Value::Map(entries) = args.first()? else {
                return None;
            };
            let found = MapKey::of(args.get(1)?).and_then(|k| entries.get(&k));
            ok(found.map_or_else(
                || args.get(2).cloned().unwrap_or(Value::Unit),
                |e| e.1.clone(),
            ))
        }
        "\0entries" => {
            let Value::Map(entries) = args.first()? else {
                return None;
            };
            ok(Value::array(
                entries
                    .values()
                    .map(|(k, v)| Value::tuple(vec![k.clone(), v.clone()]))
                    .collect(),
            ))
        }
        // `s.split_once(sep)`: the parts before and after the first `sep`.
        "\0split_once" => {
            let (Value::String(s), Value::String(sep)) = (args.first()?, args.get(1)?) else {
                return None;
            };
            ok(Value::Option(s.split_once(sep.as_str()).map(|(a, b)| {
                Box::new(Value::tuple(vec![
                    Value::String(a.to_string()),
                    Value::String(b.to_string()),
                ]))
            })))
        }
        "\0lines" => {
            let Value::String(s) = args.first()? else {
                return None;
            };
            ok(Value::array(
                s.lines().map(|l| Value::String(l.to_string())).collect(),
            ))
        }
        "\0words" => {
            let Value::String(s) = args.first()? else {
                return None;
            };
            ok(Value::array(
                s.split_whitespace()
                    .map(|w| Value::String(w.to_string()))
                    .collect(),
            ))
        }
        _ => None,
    }
}

/// What a method on a library value is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receiver {
    String,
    Array,
    Map,
    Option,
    Result,
    Number,
    Other,
}

/// The library function `value.method(…)` calls with the value first, and
/// whether the method changes the value (a statement on a place stores the
/// function's result back, like a `mut self` method).
pub fn library_method(receiver: Receiver, method: &str) -> Option<(&'static str, bool)> {
    use Receiver::*;
    let plain = |f: &'static str| Some((f, false));
    let changes = |f: &'static str| Some((f, true));
    match (receiver, method) {
        (_, "to_string") => plain("to_string"),
        (String, "len") => plain("str_len"),
        (String, "split") => plain("str_split"),
        (String, "chars") => plain("str_chars"),
        (String, "contains") => plain("str_contains"),
        (String, "trim") => plain("str_trim"),
        (String, "starts_with") => plain("str_starts_with"),
        (String, "ends_with") => plain("str_ends_with"),
        (String, "replace") => plain("str_replace"),
        (String, "to_lower") => plain("str_to_lower"),
        (String, "to_upper") => plain("str_to_upper"),
        (String, "parse_int") => plain("parse_int"),
        (String, "parse_float") => plain("parse_float"),
        (String, "split_once") => plain("\0split_once"),
        (String, "lines") => plain("\0lines"),
        (String, "words") => plain("\0words"),
        (Array, "len") => plain("len"),
        (Array, "get") => plain("get"),
        (Array, "map") => plain("map"),
        (Array, "filter") => plain("filter"),
        (Array, "reduce") => plain("reduce"),
        (Array, "join") => plain("str_join"),
        (Array, "contains") => plain("\0contains"),
        (Array, "find") => plain("\0find"),
        (Array, "any") => plain("\0any"),
        (Array, "all") => plain("\0all"),
        (Array, "enumerate") => plain("\0enumerate"),
        (Array, "zip") => plain("\0zip"),
        (Array, "slice") => plain("\0slice"),
        (Array, "sorted") => plain("\0sorted"),
        (Array, "sorted_by") => plain("\0sorted_by"),
        (Array, "reversed") => plain("\0reversed"),
        (Array, "first") => plain("\0first"),
        (Array, "last") => plain("\0last"),
        (Array, "push") => changes("push"),
        (Array, "pop") => changes("pop"),
        (Array, "set") => changes("set"),
        (Array, "sort") => changes("\0sorted"),
        (Array, "sort_by") => changes("\0sorted_by"),
        (Array, "reverse") => changes("\0reversed"),
        (Map, "len") => plain("len"),
        (Map, "get") => plain("get"),
        (Map, "has") => plain("map_has"),
        (Map, "keys") => plain("map_keys"),
        (Map, "values") => plain("map_values"),
        (Map, "entries") => plain("\0entries"),
        (Map, "get_or") => plain("\0get_or"),
        (Map, "remove") => changes("map_remove"),
        (Map, "set") => changes("set"),
        (Option | Result, "unwrap_or") => plain("\0unwrap_or"),
        (Option | Result, "map") => plain("\0map_value"),
        (Option | Result, "and_then") => plain("\0and_then"),
        (Option, "ok_or") => plain("\0ok_or"),
        (Option, "is_some") => plain("\0is_some"),
        (Option, "is_none") => plain("\0is_none"),
        (Result, "map_err") => plain("\0map_err"),
        (Result, "is_ok") => plain("\0is_ok"),
        (Result, "is_err") => plain("\0is_err"),
        (Number, "abs") => plain("abs"),
        (Number, "sqrt") => plain("sqrt"),
        (Number, "floor") => plain("floor"),
        (Number, "ceil") => plain("ceil"),
        (Number, "pow") => plain("pow"),
        (Number, "min") => plain("min"),
        (Number, "max") => plain("max"),
        (Number, "clamp") => plain("clamp"),
        _ => None,
    }
}

/// Every library method name, for any receiver.
pub const LIBRARY_METHOD_NAMES: &[&str] = &[
    "to_string",
    "len",
    "split",
    "chars",
    "contains",
    "trim",
    "starts_with",
    "ends_with",
    "replace",
    "to_lower",
    "to_upper",
    "parse_int",
    "parse_float",
    "split_once",
    "lines",
    "words",
    "get",
    "map",
    "filter",
    "reduce",
    "join",
    "find",
    "any",
    "all",
    "enumerate",
    "zip",
    "slice",
    "sorted",
    "sorted_by",
    "reversed",
    "first",
    "last",
    "push",
    "pop",
    "set",
    "sort",
    "sort_by",
    "reverse",
    "has",
    "keys",
    "values",
    "entries",
    "get_or",
    "remove",
    "unwrap_or",
    "and_then",
    "ok_or",
    "is_some",
    "is_none",
    "map_err",
    "is_ok",
    "is_err",
    "abs",
    "sqrt",
    "floor",
    "ceil",
    "pow",
    "min",
    "max",
    "clamp",
];

/// Every library function a method can call, for the compiler to give each
/// a symbol before type checking.
pub fn library_method_targets() -> Vec<&'static str> {
    use Receiver::*;
    let mut out = Vec::new();
    let names = LIBRARY_METHOD_NAMES;
    for receiver in [String, Array, Map, Option, Result, Number, Other] {
        for name in names {
            if let Some((f, _)) = library_method(receiver, name) {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
    }
    out
}

/// Names of library methods that change their receiver.
pub const CHANGING_METHODS: &[&str] =
    &["push", "pop", "set", "sort", "sort_by", "reverse", "remove"];
