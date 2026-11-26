//! Standard library module manifests — explicit `import io` / `import math` etc.

/// `(module_name, exported intrinsic names)`
pub const STD_MODULES: &[(&str, &[&str])] = &[
    (
        "io",
        &[
            "print",
            "println",
            "eprint",
            "eprintln",
            "args",
            "read_line",
            "read_file",
            "write_file",
            "append_file",
            "file_exists",
            "read_stdin",
            "read_bytes",
            "write_bytes",
        ],
    ),
    ("env", &["env_var", "current_dir"]),
    (
        "fs",
        &[
            "list_dir",
            "is_dir",
            "create_dir",
            "remove_file",
            "remove_dir",
        ],
    ),
    ("path", &["path_join", "file_name", "extension", "parent"]),
    ("process", &["exit", "run"]),
    ("time", &["now", "after", "every", "cancel", "stop"]),
    (
        "collections",
        &[
            "len",
            "range",
            "push",
            "set",
            "pop",
            "get",
            "map",
            "filter",
            "reduce",
            "map_new",
            "map_has",
            "map_remove",
            "map_keys",
            "map_values",
            "bytes",
        ],
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
            "str_chars",
            "str_join",
            "str_contains",
            "str_trim",
            "str_starts_with",
            "str_ends_with",
            "str_replace",
            "str_to_lower",
            "str_to_upper",
            "to_string",
            "parse_int",
            "parse_float",
        ],
    ),
    ("option", &["Some", "None"]),
    // Also exported by `math` (where they first lived).
    ("testing", &["assert", "assert_eq"]),
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

/// Where a program runs: each target has its own part of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    /// A command-line program: the whole library.
    #[default]
    Cli,
    /// A web page (not buildable yet): no files, processes, environment,
    /// standard input, or command-line arguments.
    Web,
}

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Target::Cli => "cli",
            Target::Web => "web",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "cli" => Some(Target::Cli),
            "web" => Some(Target::Web),
            _ => None,
        }
    }
}

thread_local! {
    static TARGET: std::cell::Cell<Target> = const { std::cell::Cell::new(Target::Cli) };
}

/// Sets the target that checking compiles for (on this thread).
pub fn set_target(target: Target) {
    TARGET.with(|t| t.set(target));
}

/// The target that checking compiles for.
pub fn target() -> Target {
    TARGET.with(std::cell::Cell::get)
}

/// Whether `target` has the library function `name` of `module`.
pub fn available_on(target: Target, module: &str, name: &str) -> bool {
    match target {
        Target::Cli => true,
        Target::Web => {
            !matches!(module, "fs" | "process" | "env")
                && !(module == "io"
                    && matches!(
                        name,
                        "args"
                            | "read_line"
                            | "read_stdin"
                            | "read_file"
                            | "write_file"
                            | "append_file"
                            | "file_exists"
                            | "read_bytes"
                            | "write_bytes"
                    ))
        }
    }
}

/// Whether `target` has any of `module`.
pub fn module_available_on(target: Target, module: &str) -> bool {
    std_module_exports(module)
        .is_some_and(|names| names.iter().any(|n| available_on(target, module, n)))
}
