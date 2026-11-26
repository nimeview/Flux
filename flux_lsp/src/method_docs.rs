//! Signatures and one-line documentation of the library's methods, for hover
//! and signature help. Which methods exist is decided by
//! `flux_std::methods::library_method`; a test checks the two agree.

use flux_std::methods::Receiver;

type Doc = (&'static str, &'static str);

/// The signature and documentation of `value.method(…)` on a library value.
pub fn method_doc(receiver: Receiver, method: &str) -> Option<Doc> {
    use Receiver::*;
    let d = |sig, doc| Some((sig, doc));
    match (receiver, method) {
        (_, "to_string") => d("x.to_string() -> String", "The value as text."),
        (String, "len") => d("s.len() -> Int", "The number of characters."),
        (String, "split") => d(
            "s.split(separator: String) -> [String]",
            "The parts between separators.",
        ),
        (String, "chars") => d("s.chars() -> [String]", "Each character as a string."),
        (String, "contains") => d(
            "s.contains(part: String) -> Bool",
            "Whether `part` occurs in the string.",
        ),
        (String, "trim") => d(
            "s.trim() -> String",
            "The string without leading and trailing whitespace.",
        ),
        (String, "starts_with") => d(
            "s.starts_with(prefix: String) -> Bool",
            "Whether the string begins with `prefix`.",
        ),
        (String, "ends_with") => d(
            "s.ends_with(suffix: String) -> Bool",
            "Whether the string ends with `suffix`.",
        ),
        (String, "replace") => d(
            "s.replace(from: String, to: String) -> String",
            "Every `from` replaced by `to`.",
        ),
        (String, "to_lower") => d("s.to_lower() -> String", "The string in lower case."),
        (String, "to_upper") => d("s.to_upper() -> String", "The string in upper case."),
        (String, "parse_int") => d(
            "s.parse_int() -> Result[Int, String]",
            "The integer the string spells, or an error.",
        ),
        (String, "parse_float") => d(
            "s.parse_float() -> Result[Float, String]",
            "The number the string spells, or an error.",
        ),
        (String, "split_once") => d(
            "s.split_once(separator: String) -> Option[(String, String)]",
            "The parts before and after the first separator.",
        ),
        (String, "lines") => d("s.lines() -> [String]", "The lines of the string."),
        (String, "words") => d(
            "s.words() -> [String]",
            "The parts between runs of whitespace.",
        ),
        (Array, "len") => d("xs.len() -> Int", "The number of elements."),
        (Array, "get") => d(
            "xs.get(index: Int) -> Option[T]",
            "The element at `index`, or `None`.",
        ),
        (Array, "map") => d("xs.map(f: fn(T) -> U) -> [U]", "Transforms each element."),
        (Array, "filter") => d(
            "xs.filter(keep: fn(T) -> Bool) -> [T]",
            "The elements `keep` accepts.",
        ),
        (Array, "reduce") => d(
            "xs.reduce(start: A, f: fn(A, T) -> A) -> A",
            "Combines the elements, starting from `start`.",
        ),
        (Array, "join") => d(
            "xs.join(separator: String) -> String",
            "The strings joined with `separator`.",
        ),
        (Array, "contains") => d(
            "xs.contains(value: T) -> Bool",
            "Whether an element equals `value`.",
        ),
        (Array, "find") => d(
            "xs.find(test: fn(T) -> Bool) -> Option[T]",
            "The first element `test` accepts.",
        ),
        (Array, "any") => d(
            "xs.any(test: fn(T) -> Bool) -> Bool",
            "Whether `test` accepts some element.",
        ),
        (Array, "all") => d(
            "xs.all(test: fn(T) -> Bool) -> Bool",
            "Whether `test` accepts every element.",
        ),
        (Array, "enumerate") => d(
            "xs.enumerate() -> [(Int, T)]",
            "Each element with its index.",
        ),
        (Array, "zip") => d(
            "xs.zip(ys: [U]) -> [(T, U)]",
            "Pairs of elements at the same index, as many as the shorter array.",
        ),
        (Array, "slice") => d(
            "xs.slice(start: Int, end: Int) -> [T]",
            "The elements from `start` up to, not including, `end`.",
        ),
        (Array, "sorted") => d(
            "xs.sorted() -> [T]",
            "A sorted copy: numbers by value, strings by code point.",
        ),
        (Array, "sorted_by") => d(
            "xs.sorted_by(key: fn(T) -> K) -> [T]",
            "A copy sorted by `key`; equal keys keep their order.",
        ),
        (Array, "reversed") => d("xs.reversed() -> [T]", "A reversed copy."),
        (Array, "first") => d("xs.first() -> Option[T]", "The first element, or `None`."),
        (Array, "last") => d("xs.last() -> Option[T]", "The last element, or `None`."),
        (Array, "push") => d("xs.push(value: T)", "Adds `value` at the end of the array."),
        (Array, "pop") => d("xs.pop()", "Removes the last element of the array."),
        (Array, "set") => d(
            "xs.set(index: Int, value: T)",
            "Replaces the element at `index`.",
        ),
        (Array, "sort") => d(
            "xs.sort()",
            "Sorts the array: numbers by value, strings by code point.",
        ),
        (Array, "sort_by") => d(
            "xs.sort_by(key: fn(T) -> K)",
            "Sorts the array by `key`; equal keys keep their order.",
        ),
        (Array, "reverse") => d("xs.reverse()", "Reverses the array."),
        (Map, "len") => d("m.len() -> Int", "The number of entries."),
        (Map, "get") => d(
            "m.get(key: K) -> Option[V]",
            "The value for `key`, or `None`.",
        ),
        (Map, "has") => d("m.has(key: K) -> Bool", "Whether the map has `key`."),
        (Map, "keys") => d("m.keys() -> [K]", "The keys, in insertion order."),
        (Map, "values") => d("m.values() -> [V]", "The values, in insertion order."),
        (Map, "entries") => d(
            "m.entries() -> [(K, V)]",
            "The (key, value) pairs, in insertion order.",
        ),
        (Map, "get_or") => d(
            "m.get_or(key: K, default: V) -> V",
            "The value for `key`, or `default`.",
        ),
        (Map, "remove") => d("m.remove(key: K)", "Removes `key` from the map."),
        (Map, "set") => d("m.set(key: K, value: V)", "Sets the value for `key`."),
        (Option, "unwrap_or") => d(
            "o.unwrap_or(default: T) -> T",
            "The value, or `default` for `None`.",
        ),
        (Result, "unwrap_or") => d(
            "r.unwrap_or(default: T) -> T",
            "The value, or `default` for an `Err`.",
        ),
        (Option, "map") => d(
            "o.map(f: fn(T) -> U) -> Option[U]",
            "`f` applied to the value, if there is one.",
        ),
        (Result, "map") => d(
            "r.map(f: fn(T) -> U) -> Result[U, E]",
            "`f` applied to the value of an `Ok`.",
        ),
        (Option, "and_then") => d(
            "o.and_then(f: fn(T) -> Option[U]) -> Option[U]",
            "`f`'s result for a value; `None` stays `None`.",
        ),
        (Result, "and_then") => d(
            "r.and_then(f: fn(T) -> Result[U, E]) -> Result[U, E]",
            "`f`'s result for an `Ok`; an `Err` passes through.",
        ),
        (Option, "ok_or") => d(
            "o.ok_or(error: E) -> Result[T, E]",
            "`Ok` with the value, or `Err(error)` for `None`.",
        ),
        (Option, "is_some") => d("o.is_some() -> Bool", "Whether there is a value."),
        (Option, "is_none") => d("o.is_none() -> Bool", "Whether there is no value."),
        (Result, "map_err") => d(
            "r.map_err(f: fn(E) -> F) -> Result[T, F]",
            "`f` applied to the error of an `Err`.",
        ),
        (Result, "is_ok") => d("r.is_ok() -> Bool", "Whether it is an `Ok`."),
        (Result, "is_err") => d("r.is_err() -> Bool", "Whether it is an `Err`."),
        (Number, "abs") => d("n.abs() -> N", "The absolute value."),
        (Number, "sqrt") => d("n.sqrt() -> Float", "The square root."),
        (Number, "floor") => d(
            "n.floor() -> Float",
            "The largest whole number not above `n`.",
        ),
        (Number, "ceil") => d(
            "n.ceil() -> Float",
            "The smallest whole number not below `n`.",
        ),
        (Number, "pow") => d(
            "n.pow(exponent: Float) -> Float",
            "`n` to the power `exponent`, as a `Float`.",
        ),
        (Number, "min") => d("n.min(other: N) -> N", "The smaller of the two."),
        (Number, "max") => d("n.max(other: N) -> N", "The larger of the two."),
        (Number, "clamp") => d(
            "n.clamp(low: N, high: N) -> N",
            "`n` limited to the range from `low` to `high`.",
        ),
        (Bytes, "len") => d("b.len() -> Int", "The number of bytes."),
        (Bytes, "slice") => d(
            "b.slice(start: Int, end: Int) -> Bytes",
            "Bytes `start` up to, not including, `end`; out of range is a runtime error.",
        ),
        (Bytes, "to_text") => d(
            "b.to_text() -> Result[String, String]",
            "The bytes as UTF-8 text, or `Err` saying where they stop being UTF-8.",
        ),
        (String, "to_bytes") => d("s.to_bytes() -> Bytes", "The UTF-8 bytes of `s`."),
        (Number, "checked_add") => d(
            "n.checked_add(m: Int) -> Option[N]",
            "`n + m` in `n`'s integer type, or `None` if it overflows.",
        ),
        (Number, "checked_sub") => d(
            "n.checked_sub(m: Int) -> Option[N]",
            "`n - m` in `n`'s integer type, or `None` if it overflows.",
        ),
        (Number, "checked_mul") => d(
            "n.checked_mul(m: Int) -> Option[N]",
            "`n * m` in `n`'s integer type, or `None` if it overflows.",
        ),
        (Number, "checked_div") => d(
            "n.checked_div(m: Int) -> Option[N]",
            "`n / m`, or `None` for division by zero or overflow.",
        ),
        (Number, "checked_rem") => d(
            "n.checked_rem(m: Int) -> Option[N]",
            "`n % m`, or `None` for division by zero.",
        ),
        (Number, "checked_neg") => d(
            "n.checked_neg() -> Option[N]",
            "`-n` in `n`'s integer type, or `None` if it overflows.",
        ),
        (Number, "checked_pow") => d(
            "n.checked_pow(exponent: Int) -> Option[N]",
            "`n` to the power `exponent`, or `None` if it overflows or `exponent` is negative.",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_std::methods::{library_method, LIBRARY_METHOD_NAMES};

    #[test]
    fn every_library_method_has_a_doc() {
        use Receiver::*;
        for receiver in [String, Array, Map, Option, Result, Number, Other] {
            for name in LIBRARY_METHOD_NAMES {
                let exists = library_method(receiver, name).is_some();
                let documented = method_doc(receiver, name).is_some();
                assert_eq!(exists, documented, "{receiver:?}.{name}");
            }
        }
    }
}
