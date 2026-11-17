//! The selected ledger a stored API writes beside its canonical body
//! (#2167).
//!
//! A graph's canonical body holds every definition, implementation and
//! export its build's sidecars gave, and its selected ledgers keep the ones
//! the policy retained: the same values. A stored API writes each of them
//! once, in the body. The selected ledger names the definitions and
//! implementations it keeps as runs of their places in the body, and each
//! path as a run of the body exports it is imported from, or whole when the
//! policy excluded or rejected it. The body and the selected ledger name
//! their re-export hops and units by key (`keyed::Tables`), and the
//! selected ledger's tables hold only the ones the body's do not.

use std::ops::Range;
#[cfg(feature = "rustc-driver")]
use std::sync::Arc;

use serde::{Deserialize, Serialize};
#[cfg(feature = "rustc-driver")]
use serde_json::value::RawValue;

use super::{keyed, layout, ledger, model, Captured, Graph};
use crate::{Error, Result};

/// Where a graph's selected ledgers sit in its canonical body.
#[derive(Clone, Default)]
pub(crate) struct Places {
    /// The re-export hops and units the body names, by key.
    tables: keyed::Tables,
    /// The kept definitions, as runs of their places in the body's.
    definitions: Box<[Range<u32>]>,
    /// The kept implementations, as runs of their places in the body's.
    implementations: Box<[Range<u32>]>,
    /// For each selected path, the place of the body export it is imported
    /// from, or `None` when the selected ledger writes it whole.
    exports: Box<[Option<u32>]>,
}

/// A selected path as the store writes it.
#[derive(Deserialize, Serialize)]
enum Export<P> {
    /// Paths imported from a run of body exports.
    Imported(Range<u32>),
    /// A path written whole.
    Path(P),
}

/// The selected ledger as the store writes it: support in source order
/// (`layout`).
#[derive(Serialize)]
struct Written<'graph> {
    universe: Option<[u8; 32]>,
    definitions: &'graph [Range<u32>],
    exports: Vec<Export<&'graph ledger::Path>>,
    implementations: &'graph [Range<u32>],
    scalar_predicate_proofs: &'graph [model::ScalarPredicateProof],
    support: &'graph [&'graph ledger::Support],
    anonymous: &'graph [ledger::AnonymousImport],
    index: &'graph [ledger::Index],
}

/// The stored selected ledger, its ledgers read only once its unit table is.
#[cfg(feature = "rustc-driver")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored<'bytes> {
    universe: Option<[u8; 32]>,
    definitions: Box<[Range<u32>]>,
    #[serde(borrow)]
    exports: &'bytes RawValue,
    implementations: Box<[Range<u32>]>,
    #[serde(borrow)]
    scalar_predicate_proofs: &'bytes RawValue,
    #[serde(borrow)]
    support: &'bytes RawValue,
    #[serde(borrow)]
    anonymous: &'bytes RawValue,
    #[serde(borrow)]
    index: &'bytes RawValue,
    #[serde(borrow)]
    reexports: &'bytes RawValue,
    #[serde(borrow)]
    units: &'bytes RawValue,
}

/// The ledgers a stored selected ledger gives, in the identity order a
/// graph keeps.
#[cfg(feature = "rustc-driver")]
pub(super) struct Read {
    pub(super) universe: Option<[u8; 32]>,
    pub(super) definitions: Arc<[model::Definition]>,
    pub(super) exports: Arc<[ledger::Path]>,
    pub(super) implementations: Arc<[model::Implementation]>,
    pub(super) scalar_predicate_proofs: Arc<[model::ScalarPredicateProof]>,
    pub(super) support: Arc<[ledger::Support]>,
    pub(super) anonymous: Arc<[ledger::AnonymousImport]>,
    pub(super) index: Arc<[ledger::Index]>,
}

impl Places {
    /// The places of the ledgers selected from a body whose hops and units
    /// `tables` names: `definitions` and `implementations` say, for each of the
    /// body's in its order, whether it is kept, and `exports` names each
    /// selected path's body export, if it is imported from one.
    pub(crate) fn new(
        tables: keyed::Tables,
        definitions: impl IntoIterator<Item = bool>,
        implementations: impl IntoIterator<Item = bool>,
        exports: Box<[Option<u32>]>,
    ) -> Result<Self> {
        Ok(Self {
            tables,
            definitions: runs(definitions)?,
            implementations: runs(implementations)?,
            exports,
        })
    }

    /// The places of a graph whose body is its selected ledgers and names
    /// their hops and units in `tables`, with its paths written whole.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn whole(
        tables: keyed::Tables,
        definitions: usize,
        implementations: usize,
        exports: usize,
    ) -> Result<Self> {
        Self::new(
            tables,
            std::iter::repeat_n(true, definitions),
            std::iter::repeat_n(true, implementations),
            std::iter::repeat_n(None, exports).collect(),
        )
    }

    /// The selected ledger of `graph`, whose body these places are in.
    pub(super) fn encode(
        &self,
        graph: &Graph<Captured>,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Vec<u8>> {
        if length(&self.definitions) != Some(graph.definitions.len())
            || length(&self.implementations) != Some(graph.implementations.len())
            || self.exports.len() != graph.exports.len()
        {
            return Err(Error::Extraction {
                code: "stored-api-places",
                reason: "the selected ledgers differ from their places in the body".into(),
            });
        }
        let mut support = super::references(&graph.support)?;
        layout::support(&mut support, &graph.definitions, &graph.implementations);
        let written = Written {
            universe: graph.universe,
            definitions: &self.definitions,
            exports: exports(&self.exports, &graph.exports),
            implementations: &self.implementations,
            scalar_predicate_proofs: &graph.scalar_predicate_proofs,
            support: &support,
            anonymous: &graph.anonymous,
            index: &graph.index,
        };
        let (encoded, taken) = self
            .tables
            .write(|| super::semantic::encode_once(&written, allocation));
        let mut encoded = encoded?;
        taken
            .close(&self.tables, &mut encoded)
            .map_err(|error| Error::Extraction {
                code: "semantic-api-encoding",
                reason: error.to_string().into_boxed_str(),
            })?;
        Ok(encoded)
    }
}

/// Read the stored selected ledger `selected` with the canonical `body` it
/// names places in.
#[cfg(feature = "rustc-driver")]
pub(super) fn read(selected: &[u8], body: &super::semantic::Sections<'_>) -> Result<Read> {
    let schema = |error: serde_json::Error| Error::Extraction {
        code: "stored-selected-schema",
        reason: error.to_string().into_boxed_str(),
    };
    let stored: Stored<'_> = crate::extract::sidecar::read_borrowed(selected).map_err(schema)?;
    let tables = keyed::Tables::of([
        (body.reexports, body.units),
        (Some(stored.reexports), Some(stored.units)),
    ])
    .map_err(schema)?
    .ok_or_else(|| Error::Extraction {
        code: "stored-selected-schema",
        reason: "the selected ledger's tables take keys its body's hold".into(),
    })?;
    tables.read(|| {
        let mut definitions = kept(
            decode::<model::Definition>(body.definitions).map_err(schema)?,
            &stored.definitions,
        )?;
        let mut implementations = kept(
            decode::<model::Implementation>(body.implementations).map_err(schema)?,
            &stored.implementations,
        )?;
        let exports = imported(
            decode::<Export<ledger::Path>>(stored.exports).map_err(schema)?,
            body.exports,
        )?;
        let mut support = decode::<ledger::Support>(stored.support).map_err(schema)?;
        layout::restore(&mut definitions, &mut implementations, &mut support);
        Ok(Read {
            universe: stored.universe,
            definitions: definitions.into(),
            exports: exports.into(),
            implementations: implementations.into(),
            scalar_predicate_proofs: decode(stored.scalar_predicate_proofs)
                .map_err(schema)?
                .into(),
            support: support.into(),
            anonymous: decode(stored.anonymous).map_err(schema)?.into(),
            index: decode(stored.index).map_err(schema)?.into(),
        })
    })
}

#[cfg(feature = "rustc-driver")]
fn decode<T: serde::de::DeserializeOwned>(value: &RawValue) -> serde_json::Result<Vec<T>> {
    crate::extract::sidecar::read(value.get().as_bytes())
}

/// The values of `body` at the places `runs` name, in order.
#[cfg(feature = "rustc-driver")]
fn kept<T>(body: Vec<T>, runs: &[Range<u32>]) -> Result<Vec<T>> {
    let mut places = runs.iter().flat_map(Clone::clone).peekable();
    let mut kept = Vec::new();
    for (place, value) in (0_u32..).zip(body) {
        if places.next_if_eq(&place).is_some() {
            kept.push(value);
        }
    }
    if places.next().is_some() {
        return Err(missing("a kept place is past the body's ledger"));
    }
    Ok(kept)
}

/// The selected paths `exports` names, each imported from its run of the
/// body's `exports`.
#[cfg(feature = "rustc-driver")]
fn imported(exports: Vec<Export<ledger::Path>>, body: &RawValue) -> Result<Vec<ledger::Path>> {
    let mut sources: Option<Vec<Option<crate::extract::Export>>> = None;
    let mut paths = Vec::new();
    for export in exports {
        let run = match export {
            Export::Path(path) => {
                paths.push(path);
                continue;
            }
            Export::Imported(run) => run,
        };
        let sources = match &mut sources {
            Some(sources) => sources,
            None => sources.insert(
                decode::<crate::extract::Export>(body)
                    .map_err(|error| Error::Extraction {
                        code: "stored-selected-schema",
                        reason: error.to_string().into_boxed_str(),
                    })?
                    .into_iter()
                    .map(Some)
                    .collect(),
            ),
        };
        for place in run {
            let path = sources
                .get_mut(place as usize)
                .and_then(Option::take)
                .and_then(crate::extract::Export::imported)
                .ok_or_else(|| missing("a path's body export is absent or unavailable"))?;
            paths.push(path);
        }
    }
    Ok(paths)
}

#[cfg(feature = "rustc-driver")]
fn missing(reason: &'static str) -> Error {
    Error::Extraction {
        code: "stored-api-places",
        reason: reason.into(),
    }
}

/// The paths of `paths` as the store writes them: a run of consecutive
/// body places as one entry.
fn exports<'graph>(
    places: &[Option<u32>],
    paths: &'graph [ledger::Path],
) -> Vec<Export<&'graph ledger::Path>> {
    let mut exports = Vec::new();
    for (place, path) in places.iter().zip(paths) {
        match (place, exports.last_mut()) {
            (Some(place), Some(Export::Imported(run))) if run.end == *place => run.end += 1,
            (Some(place), _) => exports.push(Export::Imported(*place..*place + 1)),
            (None, _) => exports.push(Export::Path(path)),
        }
    }
    exports
}

/// The runs of places whose `kept` is true.
fn runs(kept: impl IntoIterator<Item = bool>) -> Result<Box<[Range<u32>]>> {
    let mut runs: Vec<Range<u32>> = Vec::new();
    for (place, kept) in kept.into_iter().enumerate() {
        if !kept {
            continue;
        }
        let place = u32::try_from(place).map_err(|_| Error::Extraction {
            code: "stored-api-places",
            reason: "a body holds 2^32 entries or more".into(),
        })?;
        match runs.last_mut() {
            Some(run) if run.end == place => run.end += 1,
            _ => runs.push(place..place + 1),
        }
    }
    Ok(runs.into_boxed_slice())
}

/// How many places `runs` name.
fn length(runs: &[Range<u32>]) -> Option<usize> {
    runs.iter()
        .try_fold(0_usize, |sum, run| sum.checked_add(run.len()))
}
