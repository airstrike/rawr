//! Bind one unknown Attempt directory without relaxing command comparisons.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::Failure;
use crate::process::Command;

/// Only a declared path-valued argument can supply an Attempt binding.
pub(crate) struct Binding {
    store: PathBuf,
    argument: usize,
    suffix: PathBuf,
}

/// Fixed fixture root and at most one immutable, structurally checked Attempt.
pub(crate) struct Paths {
    root: PathBuf,
    attempt: Option<PathBuf>,
}

impl Binding {
    /// The argument must equal `<store>/executions/<key>/<id>/<suffix>`.
    pub(crate) fn argument(store: PathBuf, argument: usize, suffix: PathBuf) -> Self {
        Self {
            store,
            argument,
            suffix,
        }
    }
}

impl Paths {
    pub(super) fn new(root: PathBuf) -> Result<Self, Failure> {
        if !root.is_absolute()
            || fs::canonicalize(&root)
                .map_err(|_| Failure::Io)?
                .as_os_str()
                != root.as_os_str()
            || !fs::metadata(&root).map_err(|_| Failure::Io)?.is_dir()
        {
            return Err(Failure::File("fixture root is not a canonical directory"));
        }
        Ok(Self {
            root,
            attempt: None,
        })
    }

    /// The literal fixture root supplied before entering the production route.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The first verified path binding; it is not a store or source capability.
    pub(crate) fn attempt(&self) -> &Path {
        self.attempt
            .as_deref()
            .expect("this test step requires an earlier Attempt binding")
    }

    pub(super) fn bind(&mut self, binding: Binding, command: &Command) -> Result<(), Failure> {
        if self.attempt.is_some() {
            return Err(Failure::Binding("Attempt already bound"));
        }
        if !binding.store.starts_with(&self.root) || !relative(&binding.suffix) {
            return Err(Failure::Binding("invalid literal store or suffix"));
        }
        let actual = command
            .arguments
            .get(binding.argument)
            .ok_or(Failure::Binding("path argument absent"))?;
        let actual = Path::new(actual);
        let mut candidate = actual;
        for _ in binding.suffix.components() {
            candidate = candidate
                .parent()
                .ok_or(Failure::Binding("path suffix absent"))?;
        }
        if candidate.join(&binding.suffix).as_os_str() != actual.as_os_str() {
            return Err(Failure::Binding("path suffix differs"));
        }
        let prefix = binding.store.join("executions");
        let relative = candidate
            .strip_prefix(&prefix)
            .map_err(|_| Failure::Binding("path outside expected executions"))?;
        let mut components = relative.components();
        if !identifier(components.next())
            || !identifier(components.next())
            || components.next().is_some()
        {
            return Err(Failure::Binding(
                "expected exact ExecutionKey and BundleId components",
            ));
        }
        if fs::canonicalize(candidate)
            .map_err(|_| Failure::Io)?
            .as_os_str()
            != candidate.as_os_str()
        {
            return Err(Failure::Binding("Attempt path is not canonical"));
        }
        for partition in ["generated", "build", "target", "declared"] {
            let metadata =
                fs::symlink_metadata(candidate.join(partition)).map_err(|_| Failure::Io)?;
            if !metadata.is_dir() {
                return Err(Failure::Binding("Attempt partition is not a directory"));
            }
        }
        let lease = fs::symlink_metadata(candidate.join("lease")).map_err(|_| Failure::Io)?;
        if !lease.is_file() {
            return Err(Failure::Binding("Attempt lease is not a file"));
        }
        match fs::symlink_metadata(candidate.join("seal")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => return Err(Failure::Binding("Attempt is already sealed")),
            Err(_) => return Err(Failure::Io),
        }
        self.attempt = Some(candidate.to_path_buf());
        Ok(())
    }
}

pub(super) fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
}

fn identifier(component: Option<Component<'_>>) -> bool {
    let Some(Component::Normal(value)) = component else {
        return false;
    };
    let bytes = value.as_encoded_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
