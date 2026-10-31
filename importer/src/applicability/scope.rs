//! Binding-authority input for one original lexical method lookup.

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::Catalog;
use crate::api::model;
use crate::surface::identity;

/// Raw input supplied by the consuming resolver, after lexical name resolution.
/// This does not infer a prelude from the source edition.
#[derive(Clone, Debug, Serialize)]
pub struct Request {
    pub source: [u8; 32],
    pub lexical: [u8; 32],
    pub edition: Box<str>,
    pub prelude: Prelude,
    pub traits: Vec<Import>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum Prelude {
    Disabled,
    Active { module: identity::Definition },
}

/// The source resolver's exact trait identity and its resolved public Rust path.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Import {
    pub definition: identity::Definition,
    pub path: Box<[Box<str>]>,
}

/// Exact imported scope bound to one authenticated catalog and source context.
#[derive(Clone, Debug)]
pub struct Bound {
    pub(super) request: Request,
    pub(super) catalog: [u8; 32],
    fingerprint: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the source edition is unsupported")]
    Edition,
    #[error("a resolved trait path differs from its authenticated definition")]
    Trait,
    #[error("the effective source prelude has no exact authenticated domain")]
    Prelude,
    #[error("the scope identity could not be encoded")]
    Encoding,
}

impl Catalog<'_> {
    /// Locate metadata for an explicitly active core/std prelude. Calling this
    /// does not establish that the original source has that prelude in scope.
    pub fn prelude(&self, krate: &str, edition: &str) -> Option<identity::Definition> {
        let mut result = None;
        for prelude in self
            .partitions
            .values()
            .flat_map(|partition| partition.preludes.iter())
        {
            if prelude.module.unit().crate_name() == krate && prelude.edition.as_ref() == edition {
                let id = identity::Definition::of(&prelude.module);
                if result.is_some_and(|previous| previous != id) {
                    return None;
                }
                result = Some(id);
            }
        }
        result
    }

    pub fn scope(&self, mut request: Request) -> Result<Bound, Error> {
        if !matches!(request.edition.as_ref(), "2015" | "2018" | "2021" | "2024") {
            return Err(Error::Edition);
        }
        request.traits.sort();
        request.traits.dedup();
        for imported in &request.traits {
            let Some((root, path)) = imported.path.split_first() else {
                return Err(Error::Trait);
            };
            let path = path.iter().map(AsRef::as_ref).collect::<Vec<_>>();
            if self.export(root, &path) != Some(imported.definition) {
                return Err(Error::Trait);
            }
            let definition = self
                .identities
                .get(&imported.definition)
                .and_then(|id| self.definitions.get(id))
                .ok_or(Error::Trait)?;
            if !matches!(
                definition.kind(),
                model::Kind::Trait { .. } | model::Kind::Sysroot { .. }
            ) {
                return Err(Error::Trait);
            }
        }
        if let Prelude::Active { module } = &request.prelude {
            let matching = self
                .partitions
                .values()
                .flat_map(|partition| partition.preludes.iter())
                .filter(|prelude| {
                    identity::Definition::of(&prelude.module) == *module
                        && prelude.edition == request.edition
                })
                .collect::<Vec<_>>();
            if matching.is_empty() || matching.iter().any(|prelude| *prelude != matching[0]) {
                return Err(Error::Prelude);
            }
        }
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"rawr.import.applicability.scope/v1");
        fingerprint.update(self.fingerprint);
        fingerprint.update(serde_json::to_vec(&request).map_err(|_| Error::Encoding)?);
        Ok(Bound {
            request,
            catalog: self.fingerprint,
            fingerprint: fingerprint.finalize().into(),
        })
    }
}

impl Bound {
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    pub fn traits(&self) -> impl ExactSizeIterator<Item = &Import> {
        self.request.traits.iter()
    }
    pub fn edition(&self) -> &str {
        &self.request.edition
    }
    pub const fn prelude(&self) -> &Prelude {
        &self.request.prelude
    }
}
