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
