//! One sealed import context captured before shared-store or process work.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use sha2::{Digest as _, Sha256};

use crate::cargo;

const CONTEXT_SCHEMA: u32 = 5;
const LAUNCHER_POLICY_SCHEMA: u32 = 1;

/// Canonical project, Cargo-home, environment, target, and limit authority.
#[derive(Clone)]
pub struct Context {
    project: PathBuf,
    project_key: [u8; 32],
    cargo_home: PathBuf,
    /// `<target>/rawr`, holding everything derived from this project.
    rawr: PathBuf,
    environment: cargo::config::Environment,
    /// The project's compiler selected by its already-discovered Cargo config.
    /// Generic contexts need not have Cargo authority; shared runtime builds do.
    compiler: Option<cargo::config::Compiler>,
    target: Box<str>,
    limits: cargo::Limits,
    digest: [u8; 32],
    graph_budget: crate::node::GraphBudget,
    toolchain: Arc<OnceLock<Arc<crate::node::Validated<cargo::toolchain::Attestation>>>>,
}

impl Context {
    /// Capture every currently supported direct-node context fact before work.
    pub fn capture(
        project: &Path,
        environment: cargo::config::Environment,
        target: &str,
        limits: &cargo::Limits,
    ) -> Result<Self, Error> {
        Self::capture_in(
            project,
            environment,
            target,
            limits,
            &crate::node::GraphBudget::with_targets(limits.graph_targets()),
        )
    }

    pub(crate) fn capture_in(
        project: &Path,
        environment: cargo::config::Environment,
        target: &str,
        limits: &cargo::Limits,
        graph_budget: &crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        let project = canonical_directory("project", project)?;
        let project_key = project_identity(&project);
        let cargo_home = environment
            .context_cargo_home()
            .map_err(Error::Environment)?
            .ok_or(Error::MissingCargoHome)?;
        let cargo_home = canonical_directory("Cargo home", &cargo_home)?;
        if target.is_empty() || target.contains('\0') {
            return Err(Error::Target("must be nonempty and contain no NUL"));
        }
        // The target string's size is an accounting target recorded on
        // a classification-stage account, never a refusal.
        crate::classify::Account::new().exceeded(
            "import context target bytes",
            limits.string_bytes,
            target.len(),
        );
        let target: Box<str> = target.into();
        let rawr = crate::location::rawr_directory(&project, &environment);
        fs::create_dir_all(&rawr).map_err(|source| Error::Io {
            operation: "create the project's Rawr directory",
            path: rawr.clone(),
            source,
        })?;
        // Load persisted tool digests before keying what PATH resolves.
        crate::file_digest::attach(&rawr.join("digests"));
        let digest = identity(&project, &cargo_home, &environment, &target, limits, None);
        Ok(Self {
            project,
            project_key,
            cargo_home,
            rawr,
            environment,
            compiler: None,
            target,
            limits: *limits,
            digest,
            graph_budget: graph_budget.clone(),
            toolchain: Arc::new(OnceLock::new()),
        })
    }

    /// Bind the compiler selected by this request's existing Cargo authority
    /// before discovering any node. Clones then carry the same selection into
    /// both shared binding validation and runtime compilation (D644).
    fn bind_config(mut self, config: &cargo::config::Provenance) -> Self {
        self.compiler = Some(config.compiler(&self.project));
        self.digest = identity(
            &self.project,
            &self.cargo_home,
            &self.environment,
            &self.target,
            &self.limits,
            self.compiler.as_ref(),
        );
        self
    }

    pub(crate) fn capture_config(
        project: &Path,
        config: &cargo::config::Provenance,
        target: &str,
        limits: &cargo::Limits,
    ) -> Result<Self, Error> {
        Self::capture_config_in(
            project,
            config,
            target,
            limits,
            &crate::node::GraphBudget::with_targets(limits.graph_targets()),
        )
    }

    pub(crate) fn capture_config_in(
        project: &Path,
        config: &cargo::config::Provenance,
        target: &str,
        limits: &cargo::Limits,
        graph_budget: &crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        Self::capture_in(
            project,
            config.environment().clone(),
            target,
            limits,
            graph_budget,
        )
        .map(|context| context.bind_config(config))
    }

    pub(crate) fn compiler(&self) -> Option<&cargo::config::Compiler> {
        self.compiler.as_ref()
    }

    /// Open the project's typed store, `<target>/rawr/store`.
    pub fn open_store(&self) -> Result<crate::store::Store, crate::node::Error> {
        crate::store::Store::open_project(&self.rawr, self.project_key, self.graph_budget.clone())
    }

    pub(crate) fn graph_budget(&self) -> &crate::node::GraphBudget {
        &self.graph_budget
    }

    /// Bind one direct semantic recipe to this exact captured context.
    pub fn direct<K: crate::node::Value>(
        &self,
        semantic_inputs: &[u8],
        output_limit: usize,
    ) -> Result<crate::node::Direct<K>, crate::node::Error> {
        self.direct_with_parents(semantic_inputs, output_limit, &[])
    }

    /// Bind a direct recipe to current validated semantic parent receipts.
    pub fn direct_with_parents<K: crate::node::Value>(
        &self,
        semantic_inputs: &[u8],
        output_limit: usize,
        parents: &[crate::node::Parent],
    ) -> Result<crate::node::Direct<K>, crate::node::Error> {
        crate::node::Direct::capture(
            self.digest,
            semantic_inputs,
            output_limit,
            parents,
            self.limits.edges,
        )
    }

    /// Bind a discoverable recipe whose complete action is learned on a miss.
    pub fn discover<K: crate::node::Value>(
        &self,
        fixed_inputs: &[u8],
        output_limit: usize,
    ) -> Result<crate::node::Discoverable<K>, crate::node::Error> {
        self.discover_with_parents(fixed_inputs, output_limit, &[])
    }

    /// Bind a discoverable recipe to current validated semantic parents.
    pub fn discover_with_parents<K: crate::node::Value>(
        &self,
        fixed_inputs: &[u8],
        output_limit: usize,
        parents: &[crate::node::Parent],
    ) -> Result<crate::node::Discoverable<K>, crate::node::Error> {
        crate::node::Discoverable::capture(
            self.digest,
            fixed_inputs,
            output_limit,
            parents,
            self.limits.edges,
        )
    }

    /// Canonical project root included in every direct action identity.
    #[must_use]
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// Canonical Cargo home owning the versioned shared store.
    #[must_use]
    pub fn cargo_home(&self) -> &Path {
        &self.cargo_home
    }

    /// `<target>/rawr`: the project's store, build directory and runner sources.
    #[must_use]
    pub fn rawr(&self) -> &Path {
        &self.rawr
    }

    /// Exact target spelling captured before store or process work.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Comparison-only identity for recipes and diagnostics.
    #[must_use]
    pub const fn identity(&self) -> [u8; 32] {
        self.digest
    }

    /// Exact environment retained for every later governed subprocess.
    #[must_use]
    pub fn environment(&self) -> &cargo::config::Environment {
        &self.environment
    }

    /// Frozen Cargo, allocation, and process limits included in this context.
    #[must_use]
    pub const fn limits(&self) -> cargo::Limits {
        self.limits
    }

    pub(crate) fn cached_toolchain(
        &self,
    ) -> Option<Arc<crate::node::Validated<cargo::toolchain::Attestation>>> {
        self.toolchain.get().cloned()
    }

    pub(crate) fn retain_toolchain(
        &self,
        toolchain: Arc<crate::node::Validated<cargo::toolchain::Attestation>>,
    ) -> Arc<crate::node::Validated<cargo::toolchain::Attestation>> {
        match self.toolchain.set(Arc::clone(&toolchain)) {
            Ok(()) => toolchain,
            Err(_) => self.toolchain.get().cloned().unwrap_or(toolchain),
        }
    }

    #[cfg(test)]
    pub(crate) fn without_toolchain_memo(&self) -> Self {
        Self {
            project: self.project.clone(),
            project_key: self.project_key,
            cargo_home: self.cargo_home.clone(),
            rawr: self.rawr.clone(),
            environment: self.environment.clone(),
            compiler: self.compiler.clone(),
            target: self.target.clone(),
            limits: self.limits,
            digest: self.digest,
            graph_budget: self.graph_budget.clone(),
            toolchain: Arc::new(OnceLock::new()),
        }
    }
}

fn project_identity(project: &Path) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.store.project/v1");
    retain_path(&mut digest, project);
    digest.finalize().into()
}

fn identity(
    project: &Path,
    cargo_home: &Path,
    environment: &cargo::config::Environment,
    target: &str,
    limits: &cargo::Limits,
    compiler: Option<&cargo::config::Compiler>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.context/v1");
    digest.update(CONTEXT_SCHEMA.to_le_bytes());
    digest.update(LAUNCHER_POLICY_SCHEMA.to_le_bytes());
    retain_path(&mut digest, project);
    retain_path(&mut digest, cargo_home);
    match compiler {
        Some(compiler) => {
            digest.update([1]);
            match compiler.configured() {
                Some(path) => {
                    digest.update([1]);
                    retain_path(&mut digest, path);
                }
                None => digest.update([0]),
            }
        }
        None => digest.update([0]),
    }
    // Key on build-relevant variables only; children still inherit all.
    let relevant: Vec<_> = environment
        .entries()
        .filter(|(key, _)| cargo::config::projection::build_relevant(key))
        .collect();
    digest.update((relevant.len() as u64).to_le_bytes());
    for (key, value) in relevant {
        retain_bytes(&mut digest, key.as_bytes());
        let value = cargo::config::projection::keyed_value(key, value);
        retain_bytes(&mut digest, value.as_bytes());
    }
    retain_bytes(&mut digest, target.as_bytes());
    limits.bind_node_context(&mut digest);
    digest.finalize().into()
}

fn retain_path(digest: &mut Sha256, path: &Path) {
    retain_bytes(digest, path.as_os_str().as_encoded_bytes());
}

fn retain_bytes(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn canonical_directory(field: &'static str, path: &Path) -> Result<PathBuf, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect import context directory",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "must be an existing non-symlink directory",
        });
    }
    fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize import context directory",
        path: path.to_path_buf(),
        source,
    })
}

/// Independent sealed-context capture failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sealed import context requires CARGO_HOME or HOME/.cargo authority")]
    MissingCargoHome,
    #[error("sealed import context rejected its environment: {0}")]
    Environment(#[source] crate::Error),
    #[error("invalid import context {field} path `{path}`: {reason}")]
    Path {
        field: &'static str,
        path: PathBuf,
        reason: &'static str,
    },
    #[error("invalid import context target: {0}")]
    Target(&'static str),
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("import context I/O failed while attempting to {operation} at {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests;
