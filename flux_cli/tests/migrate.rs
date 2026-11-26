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

/// 0.4: `flux migrate` makes the fields of a `pub struct` `pub`, since they
/// were visible to importers before; other structs and enum variants stay.
#[test]
fn migrate_makes_the_fields_of_pub_structs_pub() {
    let src = "pub struct P {\n    x: Int,\n    pub y: Int\n}\nstruct Q {\n    z: Int\n}\npub enum E {\n    A { v: Int }\n}\n";
    let out = flux_cli::migrate::public_fields(src);
    assert_eq!(
        out,
        "pub struct P {\n    pub x: Int,\n    pub y: Int\n}\nstruct Q {\n    z: Int\n}\npub enum E {\n    A { v: Int }\n}\n"
    );
    assert_eq!(flux_cli::migrate::public_fields(&out), out);
}
