//! Project init helpers — manifest parsing lives in `flux_resolve`.

use std::fs;
use std::path::Path;

pub use flux_resolve::{
    parse_manifest, DependencySpec, PackageManifest, PackageSection, PathDependency, PathsSection,
};

/// Creates a project in `dir`: an app (`main.fl` with a reactive example) or,
/// with `lib`, a library (`lib.fl` exporting a function, plus `lib_test.fl`).
/// Refuses to touch a directory that already has a `flux.toml`, and never
/// overwrites an existing source file.
pub fn init_project(dir: &Path, name: &str, lib: bool) -> miette::Result<()> {
    let manifest_path = dir.join("flux.toml");
    if manifest_path.exists() {
        return Err(miette::miette!(
            "{} already exists; not overwriting an existing project",
            manifest_path.display()
        ));
    }
    fs::create_dir_all(dir)
        .map_err(|e| miette::miette!("failed to create {}: {e}", dir.display()))?;
    let mut manifest = PackageManifest::default_for(name);
    if lib {
        manifest.package.entry = "lib.fl".into();
    }
    fs::write(&manifest_path, manifest.to_toml())
        .map_err(|e| miette::miette!("failed to write flux.toml: {e}"))?;
    let entry_path = manifest.entry_path(&manifest_path);
    if let Some(parent) = entry_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| miette::miette!("failed to create {}: {e}", parent.display()))?;
    }
    let files: Vec<(std::path::PathBuf, String)> = if lib {
        vec![
            (entry_path.clone(), lib_template(name)),
            (
                entry_path.with_file_name("lib_test.fl"),
                lib_test_template(),
            ),
        ]
    } else {
        vec![(entry_path.clone(), app_template(name))]
    };
    for (path, contents) in files {
        if path.exists() {
            continue; // keep the user's file
        }
        fs::write(&path, contents)
            .map_err(|e| miette::miette!("failed to write {}: {e}", path.display()))?;
    }
    flux_driver::ensure_project_dirs(dir)
        .map_err(|e| miette::miette!("failed to create .flux directories: {e}"))?;
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        let _ = fs::write(gitignore, "/target\n.flux/\n");
    }
    Ok(())
}

fn app_template(name: &str) -> String {
    format!(
        "// {name}\n\nimport io {{ println }}\n\nevent Click\n\nlet mut count = 0\n\nlet doubled = count * 2\n\non Click {{\n    count = count + 1\n}}\n\nwatch doubled {{\n    println(\"doubled changed\")\n}}\n"
    )
}

fn lib_template(name: &str) -> String {
    format!(
        "// {name}: a Flux library. Another package depends on it with\n//   {name} = {{ path = \"../{name}\" }}\n// under [dependencies], then `import {name} {{ greet }}`.\nfn exclaim(text: String) -> String {{\n    text + \"!\"\n}}\n\npub fn greet(who: String) -> String {{\n    exclaim(\"hello, \" + who)\n}}\n"
    )
}

fn lib_test_template() -> String {
    "// Run with `flux test`.\nimport testing { assert_eq }\n\nimport lib { greet }\n\nfn test_greet() {\n    assert_eq(greet(\"flux\"), \"hello, flux!\")\n}\n".to_string()
}
