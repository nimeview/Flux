use flux_cli::migrate::migrate_source;

#[test]
fn migrate_counter_program() {
    let input = r#"event Click

count = 0
doubled = count * 2

on Click { count = count + 1 }
"#;
    let out = migrate_source(input);
    assert!(out.contains("let mut count = 0"));
    assert!(out.contains("let doubled = count * 2"));
}

#[test]
fn migrate_leaves_on_assignments() {
    let input = "let mut x = 0\non Tick { x = x + 1 }\n";
    let out = migrate_source(input);
    assert_eq!(out, input);
}
