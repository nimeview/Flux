use flux_syntax::{has_module_extension, with_module_extension, MODULE_EXTENSION};

#[test]
fn module_extension_constant() {
    assert_eq!(MODULE_EXTENSION, "fl");
}

#[test]
fn with_module_extension_idempotent() {
    assert_eq!(with_module_extension("main"), "main.fl");
    assert_eq!(with_module_extension("main.fl"), "main.fl");
    assert!(has_module_extension("lib.fl"));
    assert!(!has_module_extension("lib"));
}
