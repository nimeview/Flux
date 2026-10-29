//! Project-local directories (`.flux/cache`, `.flux/build`).

use std::path::{Path, PathBuf};

/// Hidden project metadata directory name.
pub const PROJECT_DIR: &str = ".flux";

/// IR compile cache subdirectory under [`PROJECT_DIR`].
pub const CACHE_DIR: &str = "cache";

/// Build artifacts subdirectory under [`PROJECT_DIR`].
pub const BUILD_DIR: &str = "build";

/// `<project_root>/.flux`
pub fn project_dir(root: &Path) -> PathBuf {
    root.join(PROJECT_DIR)
}

/// `<project_root>/.flux/cache`
pub fn cache_dir(root: &Path) -> PathBuf {
    project_dir(root).join(CACHE_DIR)
}

/// `<project_root>/.flux/build`
pub fn build_dir(root: &Path) -> PathBuf {
    project_dir(root).join(BUILD_DIR)
}

/// Create `.flux/cache` and `.flux/build` under a project root.
pub fn ensure_project_dirs(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir(root))?;
    std::fs::create_dir_all(build_dir(root))?;
    Ok(())
}
