// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! gix-discover's HEAD stage only; objects/refs and repository discovery remain.

use std::path;

use super::loose::{read, store};
use crate::input::membership::{
    host,
    manifest::{session::Session, Step},
};

/// Only this directory's reached HEAD stage passed; no repository proof yet.
pub(super) struct Checked<'directory> {
    directory: &'directory path::Path,
}

impl Checked<'_> {
    pub(super) fn directory(&self) -> &path::Path {
        self.directory
    }
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Failure {
    #[error("HEAD is missing")]
    MissingHead,
    #[error("HEAD resolved to misplaced reference {name:?}")]
    MisplacedHead { name: Vec<u8> },
    #[error("HEAD search failed: {0}")]
    Find(store::Failure),
}

pub(super) fn check<'directory>(
    directory: &'directory path::Path,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Checked<'directory>, Failure>> {
    let exists = {
        let head = super::path::file::join(directory, path::Path::new("HEAD"), session)?;
        super::predicate::exists(&head, session)?
    };
    if !exists {
        session.items(1)?;
        return Ok(Err(Failure::MissingHead));
    }
    {
        let store = store::Store::at(directory, session)?;
        match store.head(session)? {
            Ok(head) => {
                if head.name != b"HEAD" {
                    session.items(1)?;
                    return Ok(Err(Failure::MisplacedHead { name: head.name }));
                }
            }
            Err(store::Failure::Find(read::Failure::ReferenceCreation {
                relative_path, ..
            })) if relative_path == path::Path::new("HEAD") => {}
            Err(error) => {
                session.items(1)?;
                return Ok(Err(Failure::Find(error)));
            }
        }
    }
    // This lifetime boundary retires the store path and its empty packed Arc
    // before the caller can begin common-dir, objects or refs checks.
    session.items(1)?;
    Ok(Ok(Checked { directory }))
}

#[cfg(test)]
mod tests;
