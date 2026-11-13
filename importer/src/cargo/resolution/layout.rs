//! Fixed resolution geometry and its separate mutable execution owner.

use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use crate::node;
use crate::store::bundle;

const PROVISIONAL_CONFIG: &[u8] = b"[build]\nbuild-dir = \"build/provisional\"\n";
pub(in crate::cargo) const FINAL_CONFIG: &[u8] = b"[build]\nbuild-dir = \"build/final\"\n";

/// The mutable resolution namespace and its cleanup authority.
pub(crate) struct Layout {
    attempt: bundle::Attempt,
    budget: node::GraphBudget,
    paths: Paths,
}

/// Fixed path data without execution, publication or current owner authority.
pub(crate) struct Paths {
    root: PathBuf,
    provisional: Cargo,
    final_form: Cargo,
    extraction: Extraction,
    receipt: PathBuf,
    record: PathBuf,
}

/// One generated Cargo package and its distinct output roots.
pub(crate) struct Cargo {
    root: PathBuf,
    manifest: PathBuf,
    source: PathBuf,
    lock: PathBuf,
    target: PathBuf,
    build: PathBuf,
    config: PathBuf,
    config_bytes: &'static [u8],
}

/// The controls and sidecars belonging to the same resolution attempt.
pub(crate) struct Extraction {
    root: PathBuf,
    protocol: PathBuf,
    plan: PathBuf,
    source: PathBuf,
}

#[derive(Clone, Copy)]
enum Mode {
    Create,
    Observe,
}

const DECLARED_DIRECTORY: &str = "declared";
const RECORD_NAME: &str = "resolution.record";

impl Layout {
    /// Bind every output path while the original bundle reservation is live.
    pub(crate) fn bind(
        attempt: bundle::Attempt,
        budget: &node::GraphBudget,
    ) -> Result<Self, node::Error> {
        let root = attempt.root_budgeted()?;
        let paths = Paths::derive(root, Mode::Create)?;
        Ok(Self {
            attempt,
            budget: budget.clone(),
            paths,
        })
    }

    pub(crate) const fn paths(&self) -> &Paths {
        &self.paths
    }

    pub(crate) fn budget(&self) -> &node::GraphBudget {
        &self.budget
    }

    pub(crate) fn root(&self) -> &Path {
        self.paths.root()
    }

    pub(crate) const fn reference(&self) -> bundle::BundleRef {
        self.attempt.reference()
    }

    pub(crate) const fn provisional(&self) -> &Cargo {
        self.paths.provisional()
    }

    pub(crate) const fn final_form(&self) -> &Cargo {
        self.paths.final_form()
    }

    pub(crate) const fn extraction(&self) -> &Extraction {
        self.paths.extraction()
    }

    pub(crate) fn receipt(&self) -> &Path {
        self.paths.receipt()
    }

    /// Cold commands retain the original shared account while controls are mutable.
    pub(crate) fn require_configs(&self) -> Result<(), node::Error> {
        self.paths.require_configs()
    }

    /// Seal only after the caller has completed every command and output check.
    pub(crate) fn seal(self, seal: Box<[u8]>) -> Result<bundle::Pending, node::Error> {
        self.attempt.seal_budgeted(seal, &self.budget)
    }
}

impl Paths {
    /// Transfer an already validated bundle root and inspect its fixed geometry.
    /// This neither replaces complete bundle validation nor checks payload files.
    pub(crate) fn observe(root: PathBuf) -> Result<Self, node::Error> {
        if !root.is_absolute() {
            return Err(node::Error::OutsideRoot(root));
        }
        Self::derive(root, Mode::Observe)
    }

    fn derive(root: PathBuf, mode: Mode) -> Result<Self, node::Error> {
        let generated = bundle::path(&root, Path::new("generated"))?;
        let target = bundle::path(&root, Path::new("target"))?;
        let build = bundle::path(&root, Path::new("build"))?;
        let declared = bundle::path(&root, Path::new(DECLARED_DIRECTORY))?;
        let (generated, target, build, declared) = match mode {
            Mode::Create => (generated, target, build, declared),
            Mode::Observe => (
                observe_directory(generated)?,
                observe_directory(target)?,
                observe_directory(build)?,
                observe_directory(declared)?,
            ),
        };
        let provisional = Cargo::bind(&generated, &target, &build, &declared, "provisional", mode)?;
        let final_form = Cargo::bind(&generated, &target, &build, &declared, "final", mode)?;
        let extraction = Extraction::bind(&declared, mode)?;
        let receipt = bundle::path(final_form.root(), Path::new(crate::cargo::UNIVERSE_RECEIPT))?;
        let record = bundle::path(&declared, Path::new(RECORD_NAME))?;
        Ok(Self {
            root,
            provisional,
            final_form,
            extraction,
            receipt,
            record,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) const fn provisional(&self) -> &Cargo {
        &self.provisional
    }

    pub(crate) const fn final_form(&self) -> &Cargo {
        &self.final_form
    }

    pub(crate) const fn extraction(&self) -> &Extraction {
        &self.extraction
    }

    pub(crate) fn receipt(&self) -> &Path {
        &self.receipt
    }

    /// Historical replay data inside the same reserved bundle as its artifacts.
    pub(crate) fn record(&self) -> &Path {
        &self.record
    }

    pub(crate) fn require_configs(&self) -> Result<(), node::Error> {
        self.provisional.require_config()?;
        self.final_form.require_config()
    }
}

impl Cargo {
    fn bind(
        generated: &Path,
        target: &Path,
        build: &Path,
        declared: &Path,
        name: &str,
        mode: Mode,
    ) -> Result<Self, node::Error> {
        let relative = Path::new(name);
        let root = directory(generated, relative, mode)?;
        let source_root = directory(&root, Path::new("src"), mode)?;
        let manifest = bundle::path(&root, Path::new("Cargo.toml"))?;
        let source = bundle::path(&source_root, Path::new("lib.rs"))?;
        let lock = bundle::path(&root, Path::new("Cargo.lock"))?;
        let target = directory(target, relative, mode)?;
        let build = directory(build, relative, mode)?;
        let (config_name, config_bytes) = match name {
            "provisional" => ("build-provisional.toml", PROVISIONAL_CONFIG),
            "final" => ("build-final.toml", FINAL_CONFIG),
            _ => return Err(node::Error::State("unknown reserved Cargo form")),
        };
        let config = bundle::path(declared, Path::new(config_name))?;
        let config = match mode {
            Mode::Create => publish_config(config, config_bytes)?,
            Mode::Observe => config,
        };
        let cargo = Self {
            root,
            manifest,
            source,
            lock,
            target,
            build,
            config,
            config_bytes,
        };
        if matches!(mode, Mode::Observe) {
            cargo.require_config()?;
        }
        Ok(cargo)
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn manifest(&self) -> &Path {
        &self.manifest
    }

    pub(crate) fn source(&self) -> &Path {
        &self.source
    }

    pub(crate) fn lock(&self) -> &Path {
        &self.lock
    }

    pub(crate) fn target(&self) -> &Path {
        &self.target
    }

    pub(crate) fn build(&self) -> &Path {
        &self.build
    }

    pub(crate) fn config(&self) -> &Path {
        &self.config
    }

    pub(crate) const fn config_bytes(&self) -> &'static [u8] {
        self.config_bytes
    }

    pub(crate) fn require_config(&self) -> Result<(), node::Error> {
        let path = self.config.clone();
        let canonical = match fs::canonicalize(&path) {
            Ok(canonical) => canonical,
            Err(source) => {
                return Err(node::Error::Io {
                    operation: "canonicalize reserved Cargo config",
                    path,
                    source,
                })
            }
        };
        if canonical.as_os_str() != path.as_os_str() {
            return Err(node::Error::Shape(canonical));
        }
        let mut file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(source) => {
                return Err(node::Error::Io {
                    operation: "open reserved Cargo config",
                    path,
                    source,
                })
            }
        };
        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(source) => {
                return Err(node::Error::Io {
                    operation: "inspect reserved Cargo config",
                    path,
                    source,
                })
            }
        };
        if !metadata.is_file()
            || bundle::hard_linked(&metadata)
            || metadata.len() != self.config_bytes.len() as u64
        {
            return Err(node::Error::Shape(path));
        }
        let mut bytes = [0; PROVISIONAL_CONFIG.len()];
        let bytes = &mut bytes[..self.config_bytes.len()];
        if let Err(source) = file.read_exact(bytes) {
            return Err(node::Error::Io {
                operation: "read reserved Cargo config",
                path,
                source,
            });
        }
        let mut extra = [0];
        let end = loop {
            match file.read(&mut extra) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result,
            }
        };
        match end {
            Ok(0) if bytes == self.config_bytes => Ok(()),
            Ok(_) => Err(node::Error::State("reserved Cargo config bytes changed")),
            Err(source) => Err(node::Error::Io {
                operation: "finish reserved Cargo config",
                path,
                source,
            }),
        }
    }
}

fn publish_config(path: PathBuf, bytes: &[u8]) -> Result<PathBuf, node::Error> {
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(source) => {
            return Err(node::Error::Io {
                operation: "create reserved Cargo config",
                path,
                source,
            })
        }
    };
    // The bundle's one root sync orders its publication after its members,
    // and a member a crash tore fails its validation and is rebuilt (#1844).
    if let Err(source) = file.write_all(bytes) {
        return Err(node::Error::Io {
            operation: "publish reserved Cargo config",
            path,
            source,
        });
    }
    Ok(path)
}

impl Extraction {
    fn bind(declared: &Path, mode: Mode) -> Result<Self, node::Error> {
        let root = directory(declared, Path::new("extraction"), mode)?;
        let protocol = directory(&root, Path::new("protocol"), mode)?;
        let plan = bundle::path(&root, Path::new("plan.json"))?;
        let source = bundle::path(&root, Path::new("probe.rs"))?;
        Ok(Self {
            root,
            protocol,
            plan,
            source,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn protocol(&self) -> &Path {
        &self.protocol
    }

    pub(crate) fn plan(&self) -> &Path {
        &self.plan
    }

    pub(crate) fn source(&self) -> &Path {
        &self.source
    }
}

fn directory(parent: &Path, relative: &Path, mode: Mode) -> Result<PathBuf, node::Error> {
    let path = bundle::path(parent, relative)?;
    if matches!(mode, Mode::Observe) {
        return observe_directory(path);
    }
    match fs::create_dir(&path) {
        Ok(()) => Ok(path),
        Err(source) => Err(node::Error::Io {
            operation: "create reserved resolution directory",
            path,
            source,
        }),
    }
}

fn observe_directory(path: PathBuf) -> Result<PathBuf, node::Error> {
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) => {
            return Err(node::Error::Io {
                operation: "inspect fixed resolution directory",
                path,
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(node::Error::Shape(path));
    }
    let canonical = match fs::canonicalize(&path) {
        Ok(canonical) => canonical,
        Err(source) => {
            return Err(node::Error::Io {
                operation: "canonicalize fixed resolution directory",
                path,
                source,
            });
        }
    };
    if canonical.as_os_str() != path.as_os_str() {
        return Err(node::Error::Shape(canonical));
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
