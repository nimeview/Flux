use std::fs;
use std::path::PathBuf;

use flux_resolve::{load_path_dependencies, parse_manifest, DependencySpec, PackageManifest};

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("flux_manifest_{name}_{}", std::process::id()))
}

#[test]
fn parse_basic_manifest() {
    let m = parse_manifest(
        r#"
        [package]
        name = "demo"
        version = "0.2.0"
        entry = "app.fl"

        [paths]
        src = "src"
        "#,
    )
    .unwrap();
    assert_eq!(m.package.name, "demo");
    assert_eq!(m.package.version, "0.2.0");
    assert_eq!(m.package.entry, "app.fl");
    assert_eq!(m.paths.src, "src");
}

#[test]
fn parse_with_dependencies() {
    let m = parse_manifest(
        r#"
        [package]
        name = "app"
        version = "0.1.0"
        entry = "main.fl"

        [dependencies]
        utils = { path = "libs/utils" }
        "#,
    )
    .unwrap();
    assert_eq!(m.dependencies.len(), 1);
    match m.dependencies.get("utils").unwrap() {
        DependencySpec::Path { path } => assert_eq!(path, "libs/utils"),
        _ => panic!("expected path dep"),
    }
}

#[test]
fn round_trip_default() {
    let m = PackageManifest::default_for("test");
    let parsed = parse_manifest(&m.to_toml()).unwrap();
    assert_eq!(parsed.package.name, "test");
    assert_eq!(parsed.package.entry, "main.fl");
}

#[test]
fn entry_path_honors_src_directory() {
    let m = parse_manifest(
        r#"
        [package]
        name = "app"
        version = "0.1.0"
        entry = "main.fl"

        [paths]
        src = "src"
        "#,
    )
    .unwrap();
    let manifest_path = std::path::Path::new("/proj/flux.toml");
    assert_eq!(
        m.entry_path(manifest_path),
        std::path::Path::new("/proj/src/main.fl")
    );
}

#[test]
fn load_manifest_entry_under_src() {
    let root = temp_root("entry_src");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let manifest_path = root.join("flux.toml");
    fs::write(
        &manifest_path,
        r#"
        [package]
        name = "demo"
        version = "0.1.0"
        entry = "main.fl"

        [paths]
        src = "src"
        "#,
    )
    .unwrap();
    let m = PackageManifest::load(&manifest_path).unwrap();
    let entry = m.entry_path(&manifest_path);
    fs::create_dir_all(entry.parent().unwrap()).unwrap();
    fs::write(&entry, "let x = 0\n").unwrap();
    assert!(entry.ends_with("src/main.fl"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn invalid_toml_errors() {
    assert!(parse_manifest("not valid [[[").is_err());
}

#[test]
fn load_path_dependencies_uses_dep_src_root() {
    let root = temp_root("path_deps");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("libs/utils/src")).unwrap();
    fs::write(
        root.join("flux.toml"),
        r#"
[package]
name = "app"
version = "0.1.0"

[dependencies]
utils = { path = "libs/utils" }
"#,
    )
    .unwrap();
    fs::write(
        root.join("libs/utils/flux.toml"),
        r#"
[package]
name = "utils"
version = "0.1.0"

[paths]
src = "src"
"#,
    )
    .unwrap();

    let deps = load_path_dependencies(&root);
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].name, "utils");
    assert!(deps[0].module_root.ends_with("libs/utils/src"));

    let _ = fs::remove_dir_all(&root);
}
