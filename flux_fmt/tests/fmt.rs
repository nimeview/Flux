use flux_fmt::format_source;

fn assert_format_idempotent(input: &str, expected: &str) {
    let out = format_source(input).expect("format");
    assert_eq!(out, expected);
    assert_eq!(format_source(&out).expect("second pass"), out);
}

#[test]
fn short_match_stays_inline() {
    assert_format_idempotent(
        "let x=match n {\n0=>1,\n_=>2\n}\n",
        "let x = match n { 0 => 1, _ => 2 }\n",
    );
}

fn boundary_match(columns: usize) -> (String, String) {
    let empty = "    match n { 0 => \"\", _ => \"\" }";
    let value = "x".repeat(columns - empty.len());
    let expression = format!("match n {{ 0 => \"{value}\", _ => \"\" }}");
    assert_eq!(4 + expression.len(), columns);
    (expression, value)
}

#[test]
fn match_at_100_columns_including_indentation_stays_inline() {
    let (expression, _) = boundary_match(100);
    let input = format!("fn f(n: Int) -> String {{\n    {expression}\n}}\n");
    assert_format_idempotent(&input, &input);
}

#[test]
fn match_at_101_columns_including_indentation_breaks() {
    let (expression, value) = boundary_match(101);
    let input = format!("fn f(n: Int) -> String {{\n    {expression}\n}}\n");
    let expected = format!("fn f(n: Int) -> String {{\n    match n {{\n        0 => \"{value}\",\n        _ => \"\"\n    }}\n}}\n");
    assert_format_idempotent(&input, &expected);
}

#[test]
fn long_match_nested_in_a_match_and_function() {
    let value = "x".repeat(100);
    let input = format!("fn f(n: Int) -> String {{\n    match n {{ 0 => match n {{ 1 => \"{value}\", _ => \"\" }}, _ => match n {{ 2 => \"yes\", _ => \"no\" }} }}\n}}\n");
    let expected = format!("fn f(n: Int) -> String {{\n    match n {{\n        0 => match n {{\n            1 => \"{value}\",\n            _ => \"\"\n        }},\n        _ => match n {{ 2 => \"yes\", _ => \"no\" }}\n    }}\n}}\n");
    assert_format_idempotent(&input, &expected);
}

#[test]
fn long_match_keeps_guards_and_arm_commas() {
    let value = "x".repeat(100);
    let input = format!("let x = match n {{ Some(v) if v > 0 => \"{value}\", Some(v) if v == 0 => \"zero\", _ => \"other\" }}\n");
    let expected = format!("let x = match n {{\n    Some(v) if v > 0 => \"{value}\",\n    Some(v) if v == 0 => \"zero\",\n    _ => \"other\"\n}}\n");
    assert_format_idempotent(&input, &expected);
}

#[test]
fn formats_let_binding_with_newline() {
    let input = "let mut x=1\nlet y = 2";
    let out = format_source(input).expect("parse");
    assert!(out.contains("let mut x = 1"));
    assert!(out.contains("let y = 2"));
    assert!(out.ends_with('\n'));
}

#[test]
fn preserves_pub_on_exports() {
    let input = "pub let mut x = 1\npub fn f() { return 0 }\n";
    let out = format_source(input).expect("parse");
    assert!(out.contains("pub let mut x = 1"));
    assert!(out.contains("pub fn f()"));
}

#[test]
fn emits_match_guards() {
    let input = "let x = match n { Some(v) if v > 0 => v, None => 0 }\n";
    let out = format_source(input).expect("parse");
    assert!(out.contains("Some(v) if v > 0 => v"));
}

#[test]
fn parse_error_returns_err() {
    let input = "let = broken\n";
    assert!(format_source(input).is_err());
}

#[test]
fn keeps_comments_between_items() {
    let input = "// header\nlet a = 1 // trailing\nlet s = \"http://x\" // url\n\n// before fn\nfn f() -> Int {\n  2\n}\n// end\n";
    let out = format_source(input).expect("format");
    for comment in [
        "// header",
        "// trailing",
        "// url",
        "// before fn",
        "// end",
    ] {
        assert!(out.contains(comment), "lost `{comment}`:\n{out}");
    }
    assert!(out.contains("let a = 1 // trailing\n"), "{out}");
    assert!(out.contains("\"http://x\""), "{out}");
    assert_eq!(
        format_source(&out).expect("reformat"),
        out,
        "not idempotent"
    );
}

/// 0.5: comments between statements stay where they are: on their own
/// line, after a statement on its line, and before a closing brace.
#[test]
fn keeps_comments_between_statements() {
    assert_format_idempotent(
        "fn f() -> Int {\n  // inside\n  let x = 1   // one\n  if x > 0 {\n      // nested\n      return 1\n  }\n  // last\n  2\n  // end\n}\n\non Tick {\n// handler\nlet y = 1\n}\n",
        "fn f() -> Int {\n    // inside\n    let x = 1 // one\n    if x > 0 {\n        // nested\n        return 1\n    }\n    // last\n    2\n    // end\n}\n\non Tick {\n    // handler\n    let y = 1\n}\n",
    );
}

/// A comment inside an expression has no place yet: refuse, keep the file.
#[test]
fn refuses_rather_than_dropping_a_comment_inside_an_expression() {
    let input =
        "fn f() -> Int {\n    let y = [\n        1, // one\n        2,\n    ]\n    y[0]\n}\n";
    let err = format_source(input).expect_err("refuses");
    assert!(format!("{:?}", err.errors).contains("inside an expression"));
}

#[test]
fn comment_markers_inside_strings_are_not_comments() {
    let input = "let s = \"a // b\"\n";
    assert_eq!(
        format_source(input).expect("format"),
        "let s = \"a // b\"\n"
    );
}

#[test]
fn string_escapes_survive_formatting() {
    let input = "let s = \"a\\tb\\nc \\\"q\\\" \\\\ \\{x\\}\"\n";
    assert_eq!(format_source(input).expect("format"), input);
}

#[test]
fn keeps_every_branch_of_an_if_chain() {
    let input = "fn f(c: Int) -> Int {\n    let mut r = 0\n    if c > 10 {\n        r = 1\n    } else if c == 0 {\n        r = 5\n    } else {\n        r = 9\n    }\n    r\n}\n";
    assert_eq!(format_source(input).expect("format"), input);
}

#[test]
fn block_match_arms_format_on_their_own_lines() {
    let input = "let a = match parse_int(\"4\") {\n    Ok(v) => {\n        let w = v + 1\n        w\n    },\n    Err(_) => 0\n}\n";
    assert_eq!(format_source(input).expect("format"), input);
}

#[test]
fn literal_match_patterns_round_trip() {
    let input = "let a = match n { 0 => \"zero\", -1 => \"neg\", \"s\" => \"str\", true => \"t\", _ => \"other\" }\n";
    assert_eq!(format_source(input).expect("format"), input);
}

#[test]
fn round_trips_match_arms_that_leave() {
    let input = "fn f(r: Result[Int, String]) -> Result[Int, String] {\n    let x = match r {\n        Ok(v) => v,\n        Err(e) => return Err(e)\n    }\n    let y = match x {\n        0 => {\n            let z = 1\n            return Ok(z)\n        },\n        _ => x\n    }\n    Ok(y)\n}\n";
    let out = format_source(input).expect("parse");
    assert!(out.contains("Err(e) => return Err(e)"), "{out}");
    assert!(out.contains("return Ok(z)"), "{out}");
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn round_trips_the_try_operator() {
    let input = "fn f(a: String) -> Result[Int, String] {\n    let x = parse_int(a)?\n    let y = (parse_int(a)?) + 1\n    Ok(x + y + get_r(a)?.v)\n}\n";
    let out = format_source(input).expect("parse");
    assert!(out.contains("let x = parse_int(a)?"), "{out}");
    assert!(out.contains("get_r(a)?.v"), "{out}");
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn round_trips_interpolated_expressions() {
    let input = "let s = \"x={p.x} sum={n + 1} call={f(2)} lit=\\{x\\}\"\n";
    let out = format_source(input).expect("parse");
    assert!(
        out.contains("{p.x}") && out.contains("{n + 1}") && out.contains("{f(2)}"),
        "{out}"
    );
    assert!(out.contains("\\{x\\}"), "{out}");
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn round_trips_if_and_match_operands() {
    let input = "fn f(x: Int) -> Int {\n    let a = 1 + if x > 0 { 1 } else { 2 }\n    let d = match x { 1 => 3, _ => 4 } * if x == 1 { 10 } else { 1 }\n    a + d\n}\n";
    let out = format_source(input).expect("parse");
    assert!(
        out.contains("let a = 1 + if x > 0 { 1 } else { 2 }"),
        "{out}"
    );
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn round_trips_block_arms_without_a_value() {
    let input = "fn f(x: Int) {\n    let mut n = 0\n    match x {\n        0 => {\n            n = 1\n        },\n        _ => {\n            if x > 5 {\n                n = 2\n            }\n        }\n    }\n}\n";
    let out = format_source(input).expect("parse");
    // A block with one statement stays a block (0.5: `0 => n = 1` also
    // parses, and stays on its line when written so).
    assert!(!out.contains("0 => n = 1"), "{out}");
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn round_trips_indexing() {
    let input = "let a = xs[i + 1]\nlet b = grid[1][0]\nlet c = f(x)[0].name\nlet d = (if c { xs } else { ys })[0]\n";
    let out = format_source(input).expect("parse");
    for want in [
        "xs[i + 1]",
        "grid[1][0]",
        "f(x)[0].name",
        "(if c { xs } else { ys })[0]",
    ] {
        assert!(out.contains(want), "{want}: {out}");
    }
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn round_trips_nested_patterns() {
    let input = "let a = match t { Some(Num(n)) => n, Some(LParen) => 0, Ok(0) => 1, Some(Some(_)) => 2, _ => 3 }\n";
    let out = format_source(input).expect("parse");
    for want in ["Some(Num(n))", "Some(LParen)", "Ok(0)", "Some(Some(_))"] {
        assert!(out.contains(want), "{want}: {out}");
    }
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn round_trips_element_assignment() {
    let input = "fn f() {\n    let mut g = [[0]]\n    g[0][0] = 1\n    xs[i + 1] = v\n}\n";
    let out = format_source(input).expect("parse");
    assert!(
        out.contains("g[0][0] = 1") && out.contains("xs[i + 1] = v"),
        "{out}"
    );
    assert_eq!(format_source(&out).expect("reparse"), out);
}

#[test]
fn numeric_literals_keep_their_spelling() {
    let src =
        "const a = 1_000\n\nconst b = 0xff\n\nconst c = 1.5e3\n\nconst d = 1e3\n\nconst e = 2.50\n";
    let formatted = format_source(src).expect("formats");
    assert_eq!(formatted, src);
    assert_eq!(format_source(&formatted).expect("formats again"), formatted);
}

#[test]
fn trailing_commas_and_semicolons_format_away() {
    let src = "import io { println, }\nstruct P {\n    x: Int,\n}\nfn add(a: Int, b: Int,) -> Int {\n    a + b\n}\nfn main() {\n    let a = 1; let b = 2;\n    let xs = [a, b,]\n    let m = match a {\n        1 => 2,\n        _ => 3,\n    }\n    println(\"{add(a, b,)} {xs} {m}\");\n}\n";
    let want = "import io { println }\n\nstruct P {\n    x: Int\n}\n\nfn add(a: Int, b: Int) -> Int {\n    a + b\n}\n\nfn main() {\n    let a = 1\n    let b = 2\n    let xs = [a, b]\n    let m = match a { 1 => 2, _ => 3 }\n    println(\"{add(a, b)} {xs} {m}\")\n}\n";
    assert_format_idempotent(src, want);
}

#[test]
fn raw_strings_keep_their_spelling() {
    let src = "fn main() {\n    let a = r\"no {x} \\n\"\n    let b = r#\"say \"hi\"\"#\n    let c = r##\"a \"# b\"##\n}\n";
    assert_format_idempotent(src, src);
}

/// 0.5: a value `if` with statements prints as the `if` it was written as;
/// a chain with one block branch uses blocks throughout; plain chains stay
/// on one line with `else if`.
#[test]
fn value_ifs_keep_their_form() {
    assert_format_idempotent(
        "derived mode = if t < 0 { \"cold\" } else if t < 20 {\nlet mid = t * 2\n\"mild {mid}\"\n} else { \"hot\" }\n",
        "derived mode = if t < 0 {\n    \"cold\"\n} else if t < 20 {\n    let mid = t * 2\n    \"mild {mid}\"\n} else {\n    \"hot\"\n}\n",
    );
    // Statements only in the first branch: the rest of the chain uses
    // blocks too.
    assert_format_idempotent(
        "let x = if a { let t = 1\nt } else if b { 2 } else { 3 }\n",
        "let x = if a {\n    let t = 1\n    t\n} else if b {\n    2\n} else {\n    3\n}\n",
    );
    assert_format_idempotent(
        "let g = if n > 90 { \"A\" } else if n > 50 { \"B\" } else { \"C\" }\n",
        "let g = if n > 90 { \"A\" } else if n > 50 { \"B\" } else { \"C\" }\n",
    );
}

/// One-statement arms stay on the arm's line; an empty block arm is `{}`;
/// block arms may come without commas.
#[test]
fn statement_arms_stay_inline() {
    assert_format_idempotent(
        "on Hit(k) {\nmatch k {\nK::A => emit Boom(),\nK::B => n += 1,\nK::C => {}\n}\n}\n",
        "on Hit(k) {\n    match k {\n        K::A => emit Boom(),\n        K::B => n += 1,\n        K::C => {}\n    }\n}\n",
    );
    assert_format_idempotent(
        "fn f(x: Int) -> Int {\nmatch x {\n0 => {\nlet y = 1\ny\n}\n_ => 2,\n}\n}\n",
        "fn f(x: Int) -> Int {\n    match x {\n        0 => {\n            let y = 1\n            y\n        },\n        _ => 2\n    }\n}\n",
    );
}
