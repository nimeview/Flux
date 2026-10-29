//! Standard library module manifests — explicit `import io` / `import math` etc.

/// `(module_name, exported intrinsic names)`
pub const STD_MODULES: &[(&str, &[&str])] = &[
    (
        "io",
        &[
            "print",
            "println",
            "read_line",
            "read_file",
            "write_file",
            "append_file",
            "file_exists",
        ],
    ),
    (
        "collections",
        &["len", "range", "push", "pop", "map", "filter", "reduce"],
    ),
    (
        "math",
        &[
            "abs",
            "min",
            "max",
            "clamp",
            "sqrt",
            "floor",
            "ceil",
            "pow",
            "assert",
            "assert_eq",
        ],
    ),
    (
        "str",
        &[
            "str_len",
            "str_split",
            "str_join",
            "str_contains",
            "str_trim",
            "str_starts_with",
            "str_ends_with",
            "str_replace",
            "str_to_lower",
            "str_to_upper",
        ],
    ),
    ("option", &["Some", "None"]),
    ("result", &["Ok", "Err"]),
];

pub fn is_std_module(name: &str) -> bool {
    STD_MODULES.iter().any(|(m, _)| *m == name)
}

/// Top-level import path (`io`, `math`, …) or `stdlib.io`.
pub fn std_module_name(import_path: &str) -> Option<&'static str> {
    if is_std_module(import_path) {
        return STD_MODULES
            .iter()
            .find(|(m, _)| *m == import_path)
            .map(|(m, _)| *m);
    }
    import_path
        .strip_prefix("stdlib.")
        .and_then(|rest| std_module_name(rest))
}

pub fn std_module_exports(module: &str) -> Option<&'static [&'static str]> {
    STD_MODULES
        .iter()
        .find(|(m, _)| *m == module)
        .map(|(_, exports)| *exports)
}

pub fn export_in_std_module(module: &str, name: &str) -> bool {
    std_module_exports(module)
        .map(|exports| exports.contains(&name))
        .unwrap_or(false)
}
