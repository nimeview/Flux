//! Project init helpers — manifest parsing lives in `flux_resolve`.

use std::fs;
use std::path::Path;

pub use flux_resolve::{
    parse_manifest, DependencySpec, PackageManifest, PackageSection, PathDependency, PathsSection,
};

pub fn init_project(dir: &Path, name: &str) -> miette::Result<()> {
    fs::create_dir_all(dir)
        .map_err(|e| miette::miette!("failed to create {}: {e}", dir.display()))?;
    let manifest = PackageManifest::default_for(name);
    let manifest_path = dir.join("flux.toml");
    fs::write(&manifest_path, manifest.to_toml())
        .map_err(|e| miette::miette!("failed to write flux.toml: {e}"))?;
    let entry_path = manifest.entry_path(&manifest_path);
    if let Some(parent) = entry_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| miette::miette!("failed to create {}: {e}", parent.display()))?;
    }
    let main_fl = format!(
        "// {name}\n\nimport io {{ println }}\n\nevent Click\n\nlet mut count = 0\n\nlet doubled = count * 2\n\non Click {{\n    count = count + 1\n}}\n\nwatch doubled {{\n    println(\"doubled changed\")\n}}\n"
    );
    fs::write(&entry_path, main_fl)
        .map_err(|e| miette::miette!("failed to write {}: {e}", entry_path.display()))?;
    flux_driver::ensure_project_dirs(dir)
        .map_err(|e| miette::miette!("failed to create .flux directories: {e}"))?;
    let gitignore = "/target\n.flux/\n";
    let _ = fs::write(dir.join(".gitignore"), gitignore);
    Ok(())
}
