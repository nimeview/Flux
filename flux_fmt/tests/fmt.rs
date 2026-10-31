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
