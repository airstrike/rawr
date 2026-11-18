//! The alias, projection source, ABI and operation sites of a plan's
//! paths, each path's kept by the digest of what it reads (#2222).
//!
//! A path's sites are a function of the path, the definition it names and,
//! for a type alias, the definition the alias names. A plan takes a path's
//! sites from an earlier plan, of this program or another, where all three
//! have the same digest in its graph.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{
    abi_site, alias_site, definition, operation_site, projection_source_site, AbiSite, AliasSite,
    OperationSite, ProjectionSourceSite,
};
use crate::api::{self, ledger, model};

const DOMAIN: &[u8] = b"rawr.import.surface.facade.path-sites/v1";

/// What one path gives a plan's sites.
#[derive(Clone, Deserialize, Serialize)]
struct Sited {
    alias: Option<AliasSite>,
    source: Option<ProjectionSourceSite>,
    abi: Option<AbiSite>,
    operation: Option<OperationSite>,
}

/// Each path's sites one plan wrote or took, by the digest of what the
/// path reads.
#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Kept {
    #[serde(serialize_with = "entries", deserialize_with = "from_entries")]
    by_digest: BTreeMap<Digest, Site>,
}

/// What a path reads, as a digest.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Digest(#[serde(with = "crate::hex")] [u8; 32]);

/// One path's sites, and the definition it names.
#[derive(Clone, Deserialize, Serialize)]
struct Site {
    sited: Arc<Sited>,
    target: crate::surface::identity::Definition,
    /// Whether the plan that gave it wrote it.
    #[serde(skip)]
    #[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
    fresh: bool,
}

/// The sites of a plan's paths, each in selector order.
#[derive(Default)]
pub(super) struct Sites {
    pub(super) aliases: Vec<AliasSite>,
    pub(super) sources: Vec<ProjectionSourceSite>,
    pub(super) abis: Vec<AbiSite>,
    pub(super) operations: Vec<OperationSite>,
    /// How many paths' sites the plan wrote.
    pub(super) fresh: usize,
    /// Each path's sites, when the plan keeps them.
    pub(super) kept: Option<Kept>,
}

/// The sites of `graph`'s paths; with `earlier`, each path's taken from
/// there where it reads the same, and kept.
pub(super) fn sites(
    graph: &api::Graph<impl api::Phase>,
    earlier: Option<&[&Kept]>,
) -> Result<Sites, crate::surface::Error> {
    let mut sites = Sites {
        kept: earlier.map(|_| Kept::default()),
        ..Sites::default()
    };
    for export in graph.exports() {
        let ledger::Path::Imported(export) = export else {
            continue;
        };
        let sited = match (earlier, &mut sites.kept) {
            (Some(earlier), Some(kept)) => {
                let digest = read(graph, export);
                let found = earlier
                    .iter()
                    .find_map(|earlier| earlier.by_digest.get(&digest));
                let sited = match found {
                    Some(site) => Arc::clone(&site.sited),
                    None => {
                        sites.fresh += 1;
                        Arc::new(sited(graph, export)?)
                    }
                };
                kept.by_digest.insert(
                    digest,
                    Site {
                        sited: Arc::clone(&sited),
                        target: graph.surface_definition(export.target()),
                        fresh: found.is_none(),
                    },
                );
                sited
            }
            _ => {
                sites.fresh += 1;
                Arc::new(sited(graph, export)?)
            }
        };
        sites.add(&sited)?;
    }
    Ok(sites)
}

/// The digest of what `export`'s sites read of `graph`: the path, the
/// definition it names, and the one that definition names when it is a
/// type alias.
fn read(graph: &api::Graph<impl api::Phase>, export: &ledger::Export) -> Digest {
    api::reads::paused(|| {
        let mut digest = Sha256::new();
        digest.update(DOMAIN);
        digest.update(
            serde_json::to_vec(export).expect("a path encodes as the graph's canonical bytes do"),
        );
        digest.update(graph.definition_identity_digest(export.target()));
        if let Some(model::Kind::TypeAlias {
            ty: model::Type::Definition { id, .. },
            ..
        }) = definition(graph, export.target()).map(model::Definition::kind)
        {
            digest.update(graph.definition_identity_digest(id));
        }
        Digest(digest.finalize().into())
    })
}

/// The sites `export` gives.
fn sited(
    graph: &api::Graph<impl api::Phase>,
    export: &ledger::Export,
) -> Result<Sited, crate::surface::Error> {
    let alias = alias_site(graph, export)?;
    let source = match alias {
        Some(_) => projection_source_site(graph, export)?,
        None => None,
    };
    Ok(Sited {
        alias,
        source,
        abi: abi_site(graph, export)?,
        operation: operation_site(graph, export)?,
    })
}

impl Sites {
    /// Add a path's sites, each in selector order: an equal selector is the
    /// same site, which two paths to one item give.
    fn add(&mut self, sited: &Sited) -> Result<(), crate::surface::Error> {
        insert(&mut self.abis, sited.abi.as_ref(), |site| site.selector)?;
        insert(&mut self.operations, sited.operation.as_ref(), |site| {
            site.selector
        })?;
        insert(&mut self.aliases, sited.alias.as_ref(), |site| {
            site.selector
        })?;
        insert(&mut self.sources, sited.source.as_ref(), |site| {
            site.selector
        })
    }
}

/// Insert `site` into `sites` by its selector, unless one of its selector
/// is there.
fn insert<T: Clone>(
    sites: &mut Vec<T>,
    site: Option<&T>,
    selector: impl Fn(&T) -> [u8; 32],
) -> Result<(), crate::surface::Error> {
    let Some(site) = site else {
        return Ok(());
    };
    if let Err(position) =
        sites.binary_search_by(|candidate| selector(candidate).cmp(&selector(site)))
    {
        sites
            .try_reserve_exact(1)
            .map_err(|_| crate::surface::Error::Allocation)?;
        sites.insert(position, site.clone());
    }
    Ok(())
}

#[cfg(feature = "rustc-driver")]
impl Kept {
    /// The sites the plan that gave these wrote, by the owner `owner`
    /// gives the definition each path names.
    pub(crate) fn fresh_parts<O: Ord>(
        &self,
        mut owner: impl FnMut(&crate::surface::identity::Definition) -> O,
    ) -> BTreeMap<O, Self> {
        let mut parts = BTreeMap::<O, Self>::new();
        for (digest, site) in self.by_digest.iter().filter(|(_, site)| site.fresh) {
            parts
                .entry(owner(&site.target))
                .or_default()
                .by_digest
                .insert(*digest, site.clone());
        }
        parts
    }

    /// These sites with those of `later` beside them: of each definition's
    /// paths, `later`'s, then up to [`CANDIDATES`] in all.
    pub(crate) fn with(&self, later: &Self) -> Self {
        let mut by_digest = later.by_digest.clone();
        let mut per_target = BTreeMap::<crate::surface::identity::Definition, usize>::new();
        for site in later.by_digest.values() {
            *per_target.entry(site.target).or_default() += 1;
        }
        for (digest, site) in &self.by_digest {
            let count = per_target.entry(site.target).or_default();
            if *count < CANDIDATES && !by_digest.contains_key(digest) {
                *count += 1;
                by_digest.insert(*digest, site.clone());
            }
        }
        Self { by_digest }
    }
}

/// The most sites an entry keeps of one definition's paths.
#[cfg(feature = "rustc-driver")]
const CANDIDATES: usize = 4;

/// A map written as a sequence of its entries, as JSON keys are strings.
fn entries<K: Serialize, V: Serialize, S: serde::Serializer>(
    map: &BTreeMap<K, V>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(map.iter())
}

/// A map read from the sequence of entries [`entries`] writes.
fn from_entries<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Vec::<(K, V)>::deserialize(deserializer)?
        .into_iter()
        .collect())
}
