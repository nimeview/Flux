//! `flux.toml` package manifest — single parser for CLI and module resolution.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct PackageManifest {
    pub package: PackageSection,
    #[serde(default)]
    pub paths: PathsSection,
    #[serde(default)]
    pub run: RunSection,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RunSection {
    /// Enable Cranelift JIT when running via `flux run` (default: false).
    #[serde(default)]
    pub jit: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageSection {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_entry")]
    pub entry: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PathsSection {
    #[serde(default = "default_src")]
    pub src: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Path { path: String },
    Version(String),
}

/// A path dependency declared in `flux.toml`.
#[derive(Debug, Clone)]
pub struct PathDependency {
    pub name: String,
    /// Directory containing the dependency's module sources.
    pub module_root: PathBuf,
}

impl PackageManifest {
    pub fn load(path: &Path) -> miette::Result<Self> {
        let text = fs::read_to_string(path)
            .map_err(|e| miette::miette!("failed to read {}: {e}", path.display()))?;
        parse_manifest(&text)
    }

    pub fn project_root(manifest_path: &Path) -> PathBuf {
        manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn module_root(&self, manifest_path: &Path) -> PathBuf {
        Self::project_root(manifest_path).join(&self.paths.src)
    }

    pub fn entry_path(&self, manifest_path: &Path) -> PathBuf {
        self.module_root(manifest_path).join(self.entry())
    }

    pub fn default_for(name: &str) -> Self {
        Self {
            package: PackageSection {
                name: name.to_string(),
                version: default_version(),
                entry: default_entry(),
            },
            paths: PathsSection::default(),
            run: RunSection::default(),
            dependencies: HashMap::new(),
        }
    }

    pub fn to_toml(&self) -> String {
        let mut out = format!(
            "[package]\nname = \"{}\"\nversion = \"{}\"\nentry = \"{}\"\n\n[paths]\nsrc = \"{}\"\n",
            self.package.name, self.package.version, self.package.entry, self.paths.src
        );
        if !self.dependencies.is_empty() {
            out.push_str("\n[dependencies]\n");
            for (name, dep) in &self.dependencies {
                match dep {
                    DependencySpec::Path { path } => {
                        out.push_str(&format!("{name} = {{ path = \"{path}\" }}\n"));
                    }
                    DependencySpec::Version(v) => {
                        out.push_str(&format!("{name} = \"{v}\"\n"));
                    }
                }
            }
        }
        out
    }

    pub fn name(&self) -> &str {
        &self.package.name
    }

    pub fn version(&self) -> &str {
        &self.package.version
    }

    pub fn entry(&self) -> &str {
        &self.package.entry
    }

    /// Resolved path dependencies relative to `project_root`.
    pub fn path_dependencies(&self, project_root: &Path) -> Vec<PathDependency> {
        let mut deps = Vec::new();
        for (name, spec) in &self.dependencies {
            let DependencySpec::Path { path } = spec else {
                continue;
            };
            let dep_dir = project_root.join(path);
            let module_root = Self::dep_module_root(&dep_dir).unwrap_or(dep_dir);
            deps.push(PathDependency {
                name: name.clone(),
                module_root,
            });
        }
        deps
    }

    fn dep_module_root(dep_dir: &Path) -> Option<PathBuf> {
        let manifest_path = dep_dir.join("flux.toml");
        let manifest = PackageManifest::load(&manifest_path).ok()?;
        Some(dep_dir.join(manifest.paths.src))
    }
}

/// Load path dependencies from `project_root/flux.toml`.
pub fn load_path_dependencies(project_root: &Path) -> Vec<PathDependency> {
    let manifest_path = project_root.join("flux.toml");
    let Ok(manifest) = PackageManifest::load(&manifest_path) else {
        return Vec::new();
    };
    manifest.path_dependencies(project_root)
}

pub fn parse_manifest(text: &str) -> miette::Result<PackageManifest> {
    toml::from_str(text).map_err(|e| miette::miette!("invalid flux.toml: {e}"))
}

fn default_version() -> String {
    "0.1.0".to_string()
}

fn default_entry() -> String {
    "main.fl".to_string()
}

fn default_src() -> String {
    ".".to_string()
}
