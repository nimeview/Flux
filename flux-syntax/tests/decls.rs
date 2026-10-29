use flux_diagnostics::{DiagnosticBag, SourceId};
use flux_syntax::{parse_program, Item};

fn parse(src: &str) -> flux_syntax::Program {
    let mut diags = DiagnosticBag::new();
    parse_program(SourceId(0), src, &mut diags).expect("parse failed")
}

#[test]
fn parses_struct_and_enum() {
    let src = r#"
pub struct Point { x: Int, y: Int }
enum Color { Red, Green, Blue }
"#;
    let program = parse(src);
    assert!(matches!(program.items[0], Item::Struct(_)));
    assert!(matches!(program.items[1], Item::Enum(_)));
}

#[test]
fn parses_import_with_alias_and_symbols() {
    let src = "import math.utils as m { double, triple }\n";
    let program = parse(src);
    let Item::Import(imp) = &program.items[0] else {
        panic!("import");
    };
    assert_eq!(imp.path, "math.utils");
    assert_eq!(imp.alias.as_deref(), Some("m"));
    assert_eq!(imp.symbols, vec!["double", "triple"]);
}

#[test]
fn parses_event_watch_and_on() {
    let src = r#"
event click
watch { print(1) }
on click { print("clicked") }
"#;
    let program = parse(src);
    assert!(matches!(program.items[0], Item::Event(_)));
    assert!(matches!(program.items[1], Item::Watch(_)));
    assert!(matches!(program.items[2], Item::On(_)));
}

#[test]
fn parses_match_with_guard() {
    let src = "let result = match x { n if n > 0 => n, _ => 0 }\n";
    let program = parse(src);
    assert_eq!(program.items.len(), 1);
    assert!(matches!(program.items[0], Item::Let(_)));
}

#[test]
fn parses_trait_and_impl() {
    let src = r#"
trait Display { fn to_string(x: Int) -> String; }
impl Display for Int { fn to_string(self: Int) -> String { "int" } }
"#;
    let program = parse(src);
    assert!(matches!(program.items[0], Item::Trait(_)));
    assert!(matches!(program.items[1], Item::Impl(_)));
}
