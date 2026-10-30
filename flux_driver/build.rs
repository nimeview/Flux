//! Computes `FLUX_COMPILER_FINGERPRINT`: a hash of every source file that can
//! change the IR the driver caches. Cache keys and blob headers include it, so
//! a rebuilt compiler never reuses IR written by a different one.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Workspace crates whose code shapes parsing, lowering, or the IR encoding.
const CRATES: &[&str] = &[
    "flux-syntax",
    "flux_diagnostics",
    "flux_hir",
    "flux_types",
    "flux_ir",
    "flux_resolve",
    "flux_std",
    "flux_value",
    "flux_driver",
];

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "pest")
        ) {
            out.push(path);
        }
    }
}

fn main() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("flux_driver lives in the workspace root")
        .to_path_buf();
    let mut files = Vec::new();
    for krate in CRATES {
        let src = workspace.join(krate).join("src");
        println!("cargo:rerun-if-changed={}", src.display());
        println!(
            "cargo:rerun-if-changed={}",
            workspace.join(krate).join("Cargo.toml").display()
        );
        collect(&src, &mut files);
        files.push(workspace.join(krate).join("Cargo.toml"));
    }
    files.sort();
    let mut hasher = DefaultHasher::new();
    for file in &files {
        file.strip_prefix(&workspace)
            .unwrap_or(file)
            .hash(&mut hasher);
        fs::read(file).unwrap_or_default().hash(&mut hasher);
    }
    println!(
        "cargo:rustc-env=FLUX_COMPILER_FINGERPRINT={}-{:016x}",
        env!("CARGO_PKG_VERSION"),
        hasher.finish()
    );
}
