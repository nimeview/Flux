use flux_fmt::format_source;

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

#[test]
fn refuses_rather_than_dropping_a_comment_inside_an_item() {
    let input = "fn f() -> Int {\n  // inside\n  2\n}\n";
    assert!(format_source(input).is_err());
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
    assert!(
        !out.contains("0 => n = 1"),
        "a statement arm must stay a block: {out}"
    );
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
