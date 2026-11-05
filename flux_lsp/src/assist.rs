//! Editing help that works from the text, so that it keeps working while the
//! file does not compile: names in scope, the call around the cursor, imports,
//! and the standard library's signatures and documentation.

use std::collections::{HashMap, HashSet};

/// `(name, module, signature, documentation)`
pub const STD_DOCS: &[(&str, &str, &str, &str)] = &[
    ("print", "io", "print(text: String)", "Writes to standard output."),
    ("println", "io", "println(text: String)", "Writes a line to standard output."),
    ("eprint", "io", "eprint(text: String)", "Writes to standard error."),
    ("eprintln", "io", "eprintln(text: String)", "Writes a line to standard error."),
    ("args", "io", "args() -> [String]", "The program's arguments, given after `--`."),
    ("read_line", "io", "read_line() -> Result[String, String]", "One line from standard input, without the newline."),
    ("read_file", "io", "read_file(path: String) -> Result[String, String]", "A whole UTF-8 file."),
    ("write_file", "io", "write_file(path: String, text: String) -> Result[Unit, String]", "Creates or replaces a file."),
    ("append_file", "io", "append_file(path: String, text: String) -> Result[Unit, String]", "Adds to the end of a file."),
    ("file_exists", "io", "file_exists(path: String) -> Bool", "Whether the path exists."),
    ("len", "collections", "len(x: [T] | String | Map[K, V]) -> Int", "The number of elements, characters, or entries."),
    ("range", "collections", "range(start: Int, end: Int, step: Int) -> [Int]", "Numbers up to, but not including, `end`. `range(n)` and `range(a, b)` also work."),
    ("push", "collections", "push(xs: [T], value: T) -> [T]", "The array with `value` added. Write `xs = push(xs, v)`."),
    ("set", "collections", "set(xs: [T], index: Int, value: T) -> [T]", "The array or map with one element replaced. Same as `xs[i] = v`."),
    ("pop", "collections", "pop(xs: [T]) -> [T]", "The array without its last element."),
    ("get", "collections", "get(xs: [T], index: Int) -> Option[T]", "`Some(element)`, or `None` when out of range. Never fails. Also works on maps."),
    ("map", "collections", "map(xs: [T], f: fn(T) -> U) -> [U]", "Transforms each element."),
    ("filter", "collections", "filter(xs: [T], f: fn(T) -> Bool) -> [T]", "Keeps the elements for which `f` is true."),
    ("reduce", "collections", "reduce(xs: [T], init: U, f: fn(U, T) -> U) -> U", "Folds to one value. `f` must return the exact type of `init`."),
    ("map_new", "collections", "map_new() -> Map[K, V]", "An empty map. Its type comes from an annotation or the first `set`."),
    ("map_has", "collections", "map_has(m: Map[K, V], key: K) -> Bool", "Whether the key is present."),
    ("map_remove", "collections", "map_remove(m: Map[K, V], key: K) -> Map[K, V]", "The map without the key."),
    ("map_keys", "collections", "map_keys(m: Map[K, V]) -> [K]", "The keys, in order."),
    ("map_values", "collections", "map_values(m: Map[K, V]) -> [V]", "The values, in key order."),
    ("abs", "math", "abs(x: T) -> T", "The magnitude. An integer keeps its type, and the smallest signed value overflows. A float gives `Float`."),
    ("min", "math", "min(a: T, b: T) -> T", "The smaller value."),
    ("max", "math", "max(a: T, b: T) -> T", "The larger value."),
    ("clamp", "math", "clamp(x: T, low: T, high: T) -> T", "`x` limited to the range."),
    ("sqrt", "math", "sqrt(x: Float) -> Float", "The square root."),
    ("floor", "math", "floor(x: Float) -> Float", "Rounded down, as a `Float`."),
    ("ceil", "math", "ceil(x: Float) -> Float", "Rounded up, as a `Float`."),
    ("pow", "math", "pow(x: Float, y: Float) -> Float", "`x` to the power `y`, as a `Float`."),
    ("assert", "testing", "assert(condition: Bool)", "A runtime error if the condition is false."),
    ("assert_eq", "testing", "assert_eq(actual: T, expected: T)", "A runtime error if the values differ, naming both."),
    ("str_len", "str", "str_len(s: String) -> Int", "The length in Unicode scalar values."),
    ("str_split", "str", "str_split(s: String, separator: String) -> [String]", "The parts between separators."),
    ("str_chars", "str", "str_chars(s: String) -> [String]", "One string per character."),
    ("str_join", "str", "str_join(parts: [String], separator: String) -> String", "The parts joined with the separator."),
    ("str_contains", "str", "str_contains(s: String, part: String) -> Bool", "Whether `part` occurs in `s`."),
    ("str_trim", "str", "str_trim(s: String) -> String", "Without surrounding whitespace."),
    ("str_starts_with", "str", "str_starts_with(s: String, part: String) -> Bool", "Whether `s` begins with `part`."),
    ("str_ends_with", "str", "str_ends_with(s: String, part: String) -> Bool", "Whether `s` ends with `part`."),
    ("str_replace", "str", "str_replace(s: String, from: String, to: String) -> String", "With every `from` replaced."),
    ("str_to_lower", "str", "str_to_lower(s: String) -> String", "In lower case."),
    ("str_to_upper", "str", "str_to_upper(s: String) -> String", "In upper case."),
    ("to_string", "str", "to_string(value: T) -> String", "The same text interpolation gives."),
    ("parse_int", "str", "parse_int(s: String) -> Result[Int, String]", "An optional sign and digits, with no whitespace."),
    ("parse_float", "str", "parse_float(s: String) -> Result[Float, String]", "Decimal or exponent notation, with no whitespace."),
];

/// Names available everywhere: `(name, signature, documentation)`.
pub const PRELUDE: &[(&str, &str, &str)] = &[
    (
        "Ok",
        "Ok(value: T) -> Result[T, E]",
        "A successful `Result`.",
    ),
    ("Err", "Err(error: E) -> Result[T, E]", "A failed `Result`."),
    (
        "Some",
        "Some(value: T) -> Option[T]",
        "An `Option` with a value.",
    ),
    ("None", "None", "An `Option` without a value."),
];

pub const TYPE_NAMES: &[&str] = &[
    "Int", "Int8", "Int16", "Int32", "Int64", "UInt8", "UInt16", "UInt32", "UInt64", "Float",
    "Float32", "Float64", "Bool", "Char", "String", "Unit", "Option", "Result", "Map",
];

pub fn std_doc(
    name: &str,
) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    STD_DOCS.iter().find(|entry| entry.0 == name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    Ident,
    Symbol,
}

#[derive(Debug, Clone, Copy)]
struct Tok<'a> {
    kind: TokKind,
    text: &'a str,
}

/// Names and punctuation of `src`. Comments, literals, and the text of strings are
/// left out, while the code inside `{...}` in a string is kept.
fn tokens(src: &str) -> Vec<Tok<'_>> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    // One entry per open interpolation: the depth of braces inside it.
    let mut interpolations: Vec<usize> = Vec::new();
    let mut in_string = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            match c {
                b'\\' => i += 1,
                b'"' => in_string = false,
                b'{' => {
                    interpolations.push(0);
                    in_string = false;
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        match c {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => in_string = true,
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                out.push(Tok {
                    kind: TokKind::Ident,
                    text: &src[start..i],
                });
                continue;
            }
            b'0'..=b'9' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                continue;
            }
            b'{' if !interpolations.is_empty() => {
                *interpolations.last_mut().unwrap() += 1;
                out.push(symbol(src, i, 1));
            }
            b'}' if !interpolations.is_empty() => {
                if *interpolations.last().unwrap() == 0 {
                    interpolations.pop();
                    in_string = true;
                } else {
                    *interpolations.last_mut().unwrap() -= 1;
                    out.push(symbol(src, i, 1));
                }
            }
            b'|' if bytes.get(i + 1) == Some(&b'|') => {
                out.push(symbol(src, i, 2));
                i += 2;
                continue;
            }
            b'-' | b'=' if bytes.get(i + 1) == Some(&b'>') => {
                out.push(symbol(src, i, 2));
                i += 2;
                continue;
            }
            b if b.is_ascii_punctuation() => out.push(symbol(src, i, 1)),
            _ => {}
        }
        i += 1;
    }
    out
}

fn symbol(src: &str, at: usize, len: usize) -> Tok<'_> {
    Tok {
        kind: TokKind::Symbol,
        text: &src[at..at + len],
    }
}

/// Where the top-level item around `offset` starts: the last line before it that
/// begins with a letter in the first column.
fn item_start(src: &str, offset: usize) -> usize {
    let mut start = 0;
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        if at > offset {
            break;
        }
        if line.starts_with(|c: char| c.is_ascii_alphabetic()) {
            start = at;
        }
        at += line.len();
    }
    start
}

/// The local names visible at `offset`, innermost last, with the range of the item
/// they were found in.
pub fn names_in_scope(src: &str, offset: usize) -> (Vec<String>, std::ops::Range<usize>) {
    let offset = offset.min(src.len());
    let start = item_start(src, offset);
    let Some(text) = src.get(start..offset) else {
        return (Vec::new(), start..offset);
    };
    let toks = tokens(text);
    let mut names: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    let is = |at: usize, what: &str| toks.get(at).is_some_and(|t| t.text == what);
    let ident = |at: usize| toks.get(at).filter(|t| t.kind == TokKind::Ident);
    while i < toks.len() {
        let tok = toks[i];
        match tok.text {
            "{" => depth += 1,
            "}" => {
                depth = depth.saturating_sub(1);
                names.retain(|(_, d)| *d <= depth);
            }
            "let" => {
                let mut at = i + 1;
                if is(at, "mut") {
                    at += 1;
                }
                if let Some(name) = ident(at) {
                    if is(at + 1, "{") {
                        // `let Point { x, y } = p`
                        at += 2;
                        while at < toks.len() && !is(at, "}") {
                            if let Some(field) = ident(at) {
                                names.push((field.text.to_string(), depth));
                            }
                            at += 1;
                        }
                        i = at;
                    } else {
                        names.push((name.text.to_string(), depth));
                    }
                }
            }
            "for" => {
                if let Some(name) = ident(i + 1) {
                    names.push((name.text.to_string(), depth + 1));
                }
            }
            "fn" | "rfn" => {
                let mut at = i + 1;
                while at < toks.len() && !is(at, "(") {
                    at += 1;
                }
                let mut parens = 0usize;
                while at < toks.len() {
                    match toks[at].text {
                        "(" | "[" => parens += 1,
                        ")" | "]" => {
                            parens -= 1;
                            if parens == 0 {
                                break;
                            }
                        }
                        ":" if parens == 1 && at > 0 && toks[at - 1].kind == TokKind::Ident => {
                            names.push((toks[at - 1].text.to_string(), depth + 1));
                        }
                        _ => {}
                    }
                    at += 1;
                }
                i = at;
            }
            // `on Event { value ->`
            "->" if i >= 2 && is(i - 2, "{") && toks[i - 1].kind == TokKind::Ident => {
                names.push((toks[i - 1].text.to_string(), depth));
            }
            // `|a, b: T| ...`
            "|" => {
                let mut at = i + 1;
                let mut expect_name = true;
                while at < toks.len() && !is(at, "|") {
                    if is(at, ",") {
                        expect_name = true;
                    } else if expect_name {
                        if let Some(name) = ident(at) {
                            names.push((name.text.to_string(), depth));
                        }
                        expect_name = false;
                    }
                    at += 1;
                }
                i = at;
            }
            // `Some(v) if v > 0 =>`: the bindings are before any guard.
            "=>" => {
                let mut at = i;
                let mut pattern_end = i;
                let mut nested = 0usize;
                while at > 0 {
                    at -= 1;
                    match toks[at].text {
                        ")" | "}" | "]" => nested += 1,
                        "(" | "[" if nested > 0 => nested -= 1,
                        "{" if nested > 0 => nested -= 1,
                        "," | "{" if nested == 0 => {
                            at += 1;
                            break;
                        }
                        "if" if nested == 0 => pattern_end = at,
                        _ => {}
                    }
                }
                for (k, t) in toks.iter().enumerate().take(pattern_end).skip(at) {
                    let lower = t.text.starts_with(|c: char| c.is_ascii_lowercase());
                    if t.kind == TokKind::Ident
                        && lower
                        && !is(k + 1, "(")
                        && !matches!(t.text, "true" | "false")
                    {
                        names.push((t.text.to_string(), depth));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    let mut seen = HashSet::new();
    let mut out: Vec<String> = names
        .into_iter()
        .rev()
        .map(|(name, _)| name)
        .filter(|name| name != "_" && seen.insert(name.clone()))
        .collect();
    out.reverse();
    (out, start..offset)
}

/// The call around `offset`: its function's name and which argument the cursor is in.
pub fn enclosing_call(src: &str, offset: usize) -> Option<(String, usize)> {
    let offset = offset.min(src.len());
    let start = item_start(src, offset);
    let text = src.get(start..offset)?;
    let toks = tokens(text);
    struct Open {
        paren_tok: Option<usize>,
        commas: usize,
        in_closure_params: bool,
    }
    let mut stack: Vec<Open> = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        match tok.text {
            "(" => stack.push(Open {
                paren_tok: Some(i),
                commas: 0,
                in_closure_params: false,
            }),
            "[" | "{" => stack.push(Open {
                paren_tok: None,
                commas: 0,
                in_closure_params: false,
            }),
            ")" | "]" | "}" => {
                stack.pop();
            }
            "," => {
                if let Some(top) = stack.last_mut() {
                    if !top.in_closure_params {
                        top.commas += 1;
                    }
                }
            }
            "|" => {
                if let Some(top) = stack.last_mut() {
                    top.in_closure_params = !top.in_closure_params;
                }
            }
            _ => {}
        }
    }
    for open in stack.iter().rev() {
        let Some(paren) = open.paren_tok else {
            // Inside a block or array, an outer call is not what is being typed.
            return None;
        };
        if paren == 0 {
            continue;
        }
        let callee = toks[paren - 1];
        if callee.kind == TokKind::Ident && !matches!(callee.text, "if" | "while" | "match" | "in")
        {
            return Some((callee.text.to_string(), open.commas));
        }
    }
    None
}

/// The parameters of a signature such as `f(a: Int, g: fn(T) -> U) -> V`.
pub fn signature_params(signature: &str) -> Vec<String> {
    let Some(open) = signature.find('(') else {
        return Vec::new();
    };
    let mut depth = 0usize;
    let mut params = Vec::new();
    let mut current = String::new();
    for c in signature[open + 1..].chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' if depth == 0 => break,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                params.push(current.trim().to_string());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        params.push(current.trim().to_string());
    }
    params
}

/// What a file imports from each module: `None` for the whole module.
pub fn imports(src: &str) -> HashMap<String, Option<HashSet<String>>> {
    let mut out: HashMap<String, Option<HashSet<String>>> = HashMap::new();
    for line in src.lines() {
        let Some(rest) = line.trim_start().strip_prefix("import ") else {
            continue;
        };
        let rest = rest.split("//").next().unwrap_or(rest);
        let (module, list) = match rest.split_once('{') {
            Some((module, list)) => (module, Some(list.trim_end().trim_end_matches('}'))),
            None => (rest, None),
        };
        let module = module.split_whitespace().next().unwrap_or("").to_string();
        match list {
            None => {
                out.insert(module, None);
            }
            Some(list) => {
                if let Some(names) = out.entry(module).or_insert_with(|| Some(HashSet::new())) {
                    names.extend(list.split(',').map(|n| n.trim().to_string()));
                }
            }
        }
    }
    out
}

pub fn is_imported(
    imported: &HashMap<String, Option<HashSet<String>>>,
    module: &str,
    name: &str,
) -> bool {
    match imported.get(module) {
        Some(None) => true,
        Some(Some(names)) => names.contains(name),
        None => false,
    }
}

/// The edit that imports `name`: `(line, column, end column, text)`, added to a
/// selective import of the module if there is one, otherwise as a new line after
/// the last import.
pub fn import_edit(src: &str, module: &str, name: &str) -> (u32, u32, u32, String) {
    let mut last_import = None;
    let lines: Vec<&str> = src.lines().collect();
    for (number, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("import ") else {
            continue;
        };
        last_import = Some(number);
        if rest.split_whitespace().next() == Some(module) {
            if let Some(close) = line.rfind('}') {
                let before = line[..close].trim_end();
                let at = before.encode_utf16().count() as u32;
                let end = line[..close].encode_utf16().count() as u32;
                return (number as u32, at, end, format!(", {name} "));
            }
        }
    }
    let new_line = format!("import {module} {{ {name} }}\n");
    match last_import {
        Some(number) => (number as u32 + 1, 0, 0, new_line),
        None => {
            // After any comments that open the file.
            let first_code = lines
                .iter()
                .position(|l| !l.trim_start().starts_with("//") && !l.trim().is_empty())
                .unwrap_or(0);
            (first_code as u32, 0, 0, format!("{new_line}\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(src: &str) -> Vec<String> {
        let offset = src.find('@').expect("cursor");
        let text = src.replace('@', "");
        names_in_scope(&text, offset).0
    }

    #[test]
    fn scope_follows_the_code_before_the_cursor() {
        let names = scope("fn f(a: Int, g: fn(Int) -> Int) -> Int {\n    let b = 1\n    @\n    let later = 2\n}\n");
        assert_eq!(names, ["a", "g", "b"]);
    }

    #[test]
    fn a_closed_block_ends_its_names() {
        let names = scope(
            "fn f() {\n    if true {\n        let inner = 1\n    }\n    let outer = 2\n    @\n}\n",
        );
        assert_eq!(names, ["outer"]);
        let names = scope("fn f() {\n    for x in xs {\n        let y = x\n        @\n    }\n}\n");
        assert_eq!(names, ["x", "y"]);
    }

    #[test]
    fn scope_sees_every_binding_form() {
        let names = scope("on T { n ->\n    let P { x, y } = p\n    map(xs, |a, b: Int| @)\n}\n");
        assert_eq!(names, ["n", "x", "y", "a", "b"]);
        let names = scope("fn f(o: Option[Int]) -> Int {\n    match o {\n        Some(v) if v > 0 => @,\n    }\n}\n");
        assert_eq!(names, ["o", "v"]);
    }

    #[test]
    fn scope_stays_inside_the_item_and_ignores_text() {
        let names = scope("fn g(other: Int) {\n}\n\nfn f() {\n    // let hidden = 1\n    let s = \"let quoted = {1}\"\n    @\n}\n");
        assert_eq!(names, ["s"]);
    }

    #[test]
    fn scope_works_while_the_file_does_not_compile() {
        let names = scope("fn f(a: Int) {\n    let b = (1 +\n    pri@\n");
        assert_eq!(names, ["a", "b"]);
    }

    fn call(src: &str) -> Option<(String, usize)> {
        let offset = src.find('@').expect("cursor");
        enclosing_call(&src.replace('@', ""), offset)
    }

    #[test]
    fn finds_the_call_and_argument() {
        assert_eq!(call("fn f() {\n    foo(@)\n}"), Some(("foo".into(), 0)));
        assert_eq!(call("fn f() {\n    foo(1, @)\n}"), Some(("foo".into(), 1)));
        assert_eq!(
            call("fn f() {\n    foo(bar(1, 2), @)\n}"),
            Some(("foo".into(), 1))
        );
        assert_eq!(
            call("fn f() {\n    foo(bar(1, @))\n}"),
            Some(("bar".into(), 1))
        );
        assert_eq!(
            call("fn f() {\n    reduce(xs, 0, |a, b| @)\n}"),
            Some(("reduce".into(), 2))
        );
        assert_eq!(
            call("fn f() {\n    foo([1, 2], @)\n}"),
            Some(("foo".into(), 1))
        );
        assert_eq!(
            call("fn f() {\n    foo(\"a, b\", @)\n}"),
            Some(("foo".into(), 1))
        );
        assert_eq!(
            call("fn f() {\n    println(\"{foo(1, @)}\")\n}"),
            Some(("foo".into(), 1))
        );
        assert_eq!(call("fn f() {\n    foo(1)\n    @\n}"), None);
        assert_eq!(call("fn f() {\n    (1 + @)\n}"), None);
    }

    #[test]
    fn splits_parameters_at_the_top_level() {
        assert_eq!(
            signature_params("reduce(xs: [T], init: U, f: fn(U, T) -> U) -> U"),
            ["xs: [T]", "init: U", "f: fn(U, T) -> U"]
        );
        assert_eq!(
            signature_params("len(x: Map[K, V]) -> Int"),
            ["x: Map[K, V]"]
        );
        assert!(signature_params("args() -> [String]").is_empty());
    }

    #[test]
    fn reads_imports() {
        let found =
            imports("// c\nimport io { println, print }\nimport math\nimport a.b { c } // d\n");
        assert!(is_imported(&found, "io", "println"));
        assert!(!is_imported(&found, "io", "args"));
        assert!(is_imported(&found, "math", "abs"));
        assert!(is_imported(&found, "a.b", "c"));
        assert!(!is_imported(&found, "str", "str_len"));
    }

    #[test]
    fn import_edits_extend_or_add() {
        assert_eq!(
            import_edit("import io { println }\n\nfn main() {\n}\n", "io", "print"),
            (0, 19, 20, ", print ".to_string())
        );
        assert_eq!(
            import_edit(
                "import io { println }\n\nfn main() {\n}\n",
                "str",
                "str_len"
            ),
            (1, 0, 0, "import str { str_len }\n".to_string())
        );
        assert_eq!(
            import_edit("// note\n\nfn main() {\n}\n", "io", "println"),
            (2, 0, 0, "import io { println }\n\n".to_string())
        );
    }
}
