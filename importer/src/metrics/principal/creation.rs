//! Actual empty-control and directory outcomes. Metadata is not file content.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Account, Fault, Observation};
use crate::node::GraphBudget;

pub mod join;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Namespace {
    Actions,
    Content,
    Indexes,
}
impl Namespace {
    fn name(self) -> &'static str {
        match self {
            Self::Actions => "actions",
            Self::Content => "content",
            Self::Indexes => "indexes",
        }
    }
    fn parse(bytes: &[u8]) -> Option<Self> {
        match bytes {
            b"actions" => Some(Self::Actions),
            b"content" => Some(Self::Content),
            b"indexes" => Some(Self::Indexes),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Control {
    Bootstrap,
    Heavy,
    Publication,
    OwnershipReady,
    Action([u8; 32]),
    Execution([u8; 32]),
    Project([u8; 32]),
    Owner([u8; 32]),
    Enrollment([u8; 32]),
}
impl Control {
    pub fn relative(self) -> PathBuf {
        let keyed = |directory: &str, key, suffix| {
            PathBuf::from(directory).join(format!(
                "{}{}",
                crate::node::Digest::from_bytes(key).directory(),
                suffix
            ))
        };
        match self {
            Self::Bootstrap => "bootstrap.lock".into(),
            Self::Heavy => "locks/heavy.lock".into(),
            Self::Publication => "locks/publication.lock".into(),
            Self::OwnershipReady => "locks/ownership.ready".into(),
            Self::Action(key) => keyed("locks/actions", key, ".lock"),
            Self::Execution(key) => keyed("locks/executions", key, ".lock"),
            Self::Project(key) => keyed("locks/projects", key, ".lock"),
            Self::Owner(key) => keyed("locks/owners", key, ".lock"),
            Self::Enrollment(key) => keyed("locks/projects", key, ".pending"),
        }
    }
    pub(super) fn parse(path: &Path) -> Option<Self> {
        let bytes = path.as_os_str().as_encoded_bytes();
        match bytes {
            b"bootstrap.lock" => return Some(Self::Bootstrap),
            b"locks/heavy.lock" => return Some(Self::Heavy),
            b"locks/publication.lock" => return Some(Self::Publication),
            b"locks/ownership.ready" => return Some(Self::OwnershipReady),
            _ => {}
        }
        let mut parts = bytes.split(|byte| *byte == b'/');
        if parts.next()? != b"locks" {
            return None;
        }
        let directory = parts.next()?;
        let name = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        if directory == b"projects" {
            if let Some(key) = name
                .strip_suffix(b".pending")
                .and_then(super::retirement::digest)
            {
                return Some(Self::Enrollment(key));
            }
        }
        let key = super::retirement::digest(name.strip_suffix(b".lock")?)?;
        match directory {
            b"actions" => Some(Self::Action(key)),
            b"executions" => Some(Self::Execution(key)),
            b"projects" => Some(Self::Project(key)),
            b"owners" => Some(Self::Owner(key)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Directory {
    Fixed(u8),
    Kind { namespace: Namespace, kind: u8 },
    Action { kind: u8, storage: u16 },
    Content { kind: u8, schema: u32 },
    Index { kind: u8, lookup: [u8; 32] },
    Execution([u8; 32]),
}
impl Directory {
    pub fn relative(self) -> Option<PathBuf> {
        let kind = |code| crate::node::Kind::from_code(code).map(|kind| kind.directory());
        Some(match self {
            Self::Fixed(index) => {
                PathBuf::from(*crate::store::FIXED_DIRECTORIES.get(index as usize)?)
            }
            Self::Kind {
                namespace,
                kind: code,
            } => PathBuf::from(namespace.name()).join(kind(code)?),
            Self::Action {
                kind: code,
                storage,
            } => PathBuf::from("actions")
                .join(kind(code)?)
                .join(format!("{storage:04x}")),
            Self::Content { kind: code, schema } => PathBuf::from("content")
                .join(kind(code)?)
                .join(format!("{schema:08x}")),
            Self::Index { kind: code, lookup } => PathBuf::from("indexes")
                .join(kind(code)?)
                .join(crate::node::Digest::from_bytes(lookup).directory()),
            Self::Execution(key) => {
                PathBuf::from("executions").join(crate::node::Digest::from_bytes(key).directory())
            }
        })
    }
    pub(super) fn parse(path: &Path) -> Option<Self> {
        let bytes = path.as_os_str().as_encoded_bytes();
        if let Some(index) = crate::store::FIXED_DIRECTORIES
            .iter()
            .position(|fixed| fixed.as_bytes() == bytes)
        {
            return Some(Self::Fixed(index as u8));
        }
        let mut parts = bytes.split(|byte| *byte == b'/');
        let first = parts.next()?;
        let second = parts.next()?;
        let third = parts.next();
        if parts.next().is_some() {
            return None;
        }
        if first == b"executions" && third.is_none() {
            return super::retirement::digest(second).map(Self::Execution);
        }
        let namespace = Namespace::parse(first)?;
        let kind = super::retirement::kind(second)?;
        let Some(last) = third else {
            return Some(Self::Kind { namespace, kind });
        };
        match namespace {
            Namespace::Actions if last.len() == 4 => Some(Self::Action {
                kind,
                storage: super::retirement::hexadecimal(last)? as u16,
            }),
            Namespace::Content if last.len() == 8 => Some(Self::Content {
                kind,
                schema: super::retirement::hexadecimal(last)?,
            }),
            Namespace::Indexes => Some(Self::Index {
                kind,
                lookup: super::retirement::digest(last)?,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Control(Control),
    Directory(Directory),
    CargoParent,
    StoreRoot,
}
impl Target {
    pub fn relative(self) -> Option<PathBuf> {
        match self {
            Self::Control(control) => Some(control.relative()),
            Self::Directory(directory) => directory.relative(),
            Self::CargoParent => Some("rawr".into()),
            Self::StoreRoot => Some("rawr/v1".into()),
        }
    }
    pub fn directory(self) -> bool {
        !matches!(self, Self::Control(_))
    }
    pub(super) fn cargo_home(self) -> bool {
        matches!(self, Self::CargoParent | Self::StoreRoot)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    CargoHome,
    Bootstrap,
    Fixed,
    Keyed,
    Migration,
    Directory,
    OwnershipDirectory,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Created,
    Contended,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sync {
    File,
    Parent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "creation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Started {
        attempt: u64,
        publication: Option<u64>,
        root: [u8; 32],
        relative: [u8; 32],
        target: Target,
        source: Source,
    },
    Create {
        attempt: u64,
        outcome: Outcome,
    },
    Adopted {
        attempt: u64,
        mode: u32,
        bytes: Option<u64>,
    },
    Synced {
        attempt: u64,
        scope: Sync,
        succeeded: bool,
    },
    Finished {
        attempt: u64,
        succeeded: bool,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct Context<'a> {
    root: &'a Path,
    graph: &'a GraphBudget,
    publication: Option<u64>,
    source: Source,
}
impl<'a> Context<'a> {
    pub(crate) fn unleased(root: &'a Path, graph: &'a GraphBudget, source: Source) -> Self {
        Self {
            root,
            graph,
            publication: None,
            source,
        }
    }
    pub(crate) fn published(
        publication: &'a crate::store::Publication<'_>,
        source: Source,
    ) -> Self {
        Self {
            root: publication.root(),
            graph: publication.graph(),
            publication: Some(publication.observation()),
            source,
        }
    }
    pub(crate) fn start(self, path: &Path, directory: bool) -> Attempt<'a> {
        let mut attempt = Attempt {
            token: 0,
            graph: self.graph,
            succeeded: Cell::new(false),
        };
        if !crate::metrics::principal_enabled() {
            return attempt;
        }
        attempt.token = super::next();
        if attempt.token == 0 {
            crate::metrics::record_principal(&Observation {
                event: Err(Fault::Exhausted),
                budget: Account::Graph(self.graph),
            });
            return attempt;
        }
        let event = (|| {
            if attempt.token == 0 {
                return Err(Fault::Exhausted);
            }
            let relative = path.strip_prefix(self.root).map_err(|_| Fault::Identity)?;
            let target = if self.source == Source::CargoHome {
                match relative.as_os_str().as_encoded_bytes() {
                    b"rawr" if directory => Target::CargoParent,
                    b"rawr/v1" if directory => Target::StoreRoot,
                    _ => return Err(Fault::Identity),
                }
            } else if directory {
                Target::Directory(Directory::parse(relative).ok_or(Fault::Identity)?)
            } else {
                Target::Control(Control::parse(relative).ok_or(Fault::Identity)?)
            };
            if !compatible(target, self.source, self.publication) {
                return Err(Fault::Identity);
            }
            for size in [
                self.root.as_os_str().as_encoded_bytes().len(),
                relative.as_os_str().as_encoded_bytes().len(),
            ] {
                self.graph.hash(size).map_err(|_| Fault::Budget)?;
            }
            Ok(Event::Started {
                attempt: attempt.token,
                publication: self.publication,
                root: super::path_digest(self.root),
                relative: super::path_digest(relative),
                target,
                source: self.source,
            })
        })();
        attempt.emit(event);
        attempt
    }
}

pub(crate) struct Attempt<'a> {
    token: u64,
    graph: &'a GraphBudget,
    succeeded: Cell<bool>,
}
impl Attempt<'_> {
    pub(crate) fn created(&self, outcome: Outcome) {
        self.emit(Ok(Event::Create {
            attempt: self.token,
            outcome,
        }));
    }
    pub(crate) fn adopted(&self, metadata: &std::fs::Metadata) {
        if self.token == 0 {
            return;
        }
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::MetadataExt as _;
            metadata.mode() & 0o7777
        };
        #[cfg(not(unix))]
        let mode = 0;
        self.emit(Ok(Event::Adopted {
            attempt: self.token,
            mode,
            bytes: metadata.is_file().then(|| metadata.len()),
        }));
    }
    pub(crate) fn synced(&self, scope: Sync, succeeded: bool) {
        self.emit(Ok(Event::Synced {
            attempt: self.token,
            scope,
            succeeded,
        }));
    }
    pub(crate) fn completed(&self, succeeded: bool) {
        self.succeeded.set(succeeded);
    }
    fn emit(&self, event: Result<Event, Fault>) {
        if self.token != 0 {
            crate::metrics::record_principal(&Observation {
                event: event.map(|change| super::Event::Creation { change }),
                budget: Account::Graph(self.graph),
            });
        }
    }
}
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        self.emit(Ok(Event::Finished {
            attempt: self.token,
            succeeded: self.succeeded.get(),
        }));
    }
}

pub(super) fn compatible(target: Target, source: Source, publication: Option<u64>) -> bool {
    if publication == Some(0) {
        return false;
    }
    match (target, source) {
        (Target::CargoParent | Target::StoreRoot, Source::CargoHome)
        | (Target::Control(Control::Bootstrap), Source::Bootstrap)
        | (Target::Control(Control::Heavy | Control::Publication), Source::Fixed) => {
            publication.is_none()
        }
        (Target::Control(Control::Action(_) | Control::Execution(_)), Source::Keyed) => true,
        (
            Target::Control(
                Control::OwnershipReady
                | Control::Project(_)
                | Control::Owner(_)
                | Control::Enrollment(_)
                | Control::Action(_)
                | Control::Execution(_),
            ),
            Source::Migration,
        ) => publication.is_some(),
        (Target::Directory(Directory::Fixed(_)), Source::Directory) => true,
        (Target::Directory(Directory::Fixed(index)), Source::OwnershipDirectory) => {
            crate::store::FIXED_DIRECTORIES.get(index as usize) == Some(&"locks/owners")
                && publication.is_some()
        }
        (Target::Directory(_), Source::Directory) => publication.is_some(),
        _ => false,
    }
}
