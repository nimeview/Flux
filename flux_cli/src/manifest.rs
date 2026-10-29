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
    let main_fl = format!("// {name}\n\nlet mut x = 0\n");
    fs::write(&entry_path, main_fl)
        .map_err(|e| miette::miette!("failed to write {}: {e}", entry_path.display()))?;
    flux_driver::ensure_project_dirs(dir)
        .map_err(|e| miette::miette!("failed to create .flux directories: {e}"))?;
    let gitignore = "/target\n.flux/\n";
    let _ = fs::write(dir.join(".gitignore"), gitignore);
    Ok(())
}
