//! Each facade's trait headers and impl headers (D604), written one unit
//! at a time and kept under what each read (#2222).
//!
//! A unit is a trait Rawr code names at a path, an impl, or the opaque
//! types one function gives. Its headers are a function of what it reads
//! of the graph and of the facade it is for (`surface::headers`), so a
//! plan takes a unit's headers from an earlier plan, of this program or
//! another, wherever the unit's own digest and every read's digest are the
//! same in this one.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{parameters, ty, ImplBound, ImplHeader, ImplProjection, TraitHeader, Type};
use crate::api::{self, model, recorded};
use crate::surface::headers::{self as written, Key};

/// One unit of a facade's headers.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Unit {
    /// A trait Rawr code names, at its path relative to the root.
    Trait {
        path: Box<str>,
        trait_id: api::identity::Definition,
    },
    /// An impl of a trait.
    Implementation(api::identity::Definition),
    /// The opaque types a function gives, each an impl by its bounds.
    Opaque(api::identity::Definition),
}

/// What one unit gives its facade.
#[derive(Clone, Deserialize, Serialize)]
pub(crate) enum Headed {
    /// A trait's header: none when the facade can't name the trait, and
    /// none inside when it can't write the header's types.
    Trait(Option<Option<TraitHeader>>),
    /// An impl's header, or the headers of a function's opaque types.
    Impls(Box<[ImplHeader]>),
}

/// The units plans wrote, each under what it read.
pub(crate) type Kept = recorded::Table<Unit, Key, Arc<Headed>>;

impl Unit {
    /// The item whose crate keeps the unit's headers.
    #[cfg_attr(not(feature = "rustc-driver"), allow(dead_code))]
    pub(crate) const fn owner(&self) -> &api::identity::Definition {
        match self {
            Self::Trait { trait_id, .. } => trait_id,
            Self::Implementation(id) | Self::Opaque(id) => id,
        }
    }
}

/// The headers of one plan's facades: each unit taken from `earlier`
/// where it reads the same, else written and gathered for a later plan.
pub(super) struct Writer<'a, 'g, P: api::Phase> {
    models: written::Models<'g, P>,
    /// Each impl and each function that gives an opaque type, with its
    /// own digest: the units every facade has besides its traits.
    items: Vec<(Unit, api::reads::Digest)>,
    /// The earlier units to take from, and what this plan's give; none
    /// when a plan keeps no units.
    kept: Option<Keeping<'a>>,
    /// How many units this plan wrote.
    fresh: usize,
}

/// The units a plan takes from, and those it gathers.
struct Keeping<'a> {
    earlier: Vec<&'a Kept>,
    gathered: recorded::Gathered<Unit, Key, Arc<Headed>>,
}

impl<'a, 'g, P: api::Phase> Writer<'a, 'g, P> {
    /// The headers of `graph`'s facades; with `earlier`, taking each unit
    /// kept there that reads the same.
    pub(super) fn new(graph: &'g api::Graph<P>, earlier: Option<Vec<&'a Kept>>) -> Self {
        let items = api::reads::paused(|| {
            let implementations =
                graph
                    .implementations()
                    .enumerate()
                    .map(|(index, implementation)| {
                        (
                            Unit::Implementation(implementation.id().clone()),
                            graph.implementation_digest(index),
                        )
                    });
            let opaque = graph
                .definitions()
                .enumerate()
                .filter(|(_, definition)| gives_opaque(definition))
                .map(|(index, definition)| {
                    (
                        Unit::Opaque(definition.id().clone()),
                        graph.definition_digest(index),
                    )
                });
            implementations.chain(opaque).collect()
        });
        Self {
            models: written::Models::new(graph),
            items,
            kept: earlier.map(|earlier| Keeping {
                earlier,
                gathered: recorded::Gathered::default(),
            }),
            fresh: 0,
        }
    }

    /// The trait headers of `traits`, each a path relative to the root and
    /// the trait's identity, in path order, and the headers of every impl
    /// whose types the facade declaring `declared` can write. `opaque`
    /// gives the number of parameters it declares each opaque type with.
    pub(super) fn facade(
        &mut self,
        declared: &BTreeSet<[u8; 32]>,
        opaque: &BTreeMap<[u8; 32], usize>,
        traits: &[(Box<str>, [u8; 32])],
    ) -> Result<(Vec<TraitHeader>, Vec<ImplHeader>), crate::surface::Error> {
        let headers = self.models.facade(declared, opaque);
        let check = self
            .kept
            .as_ref()
            .map(|keeping| recorded::Check::new(&keeping.earlier));
        let traits = traits
            .iter()
            .filter_map(|(path, identity)| {
                let trait_id = self.models.id(identity)?;
                let mut own = Sha256::new();
                own.update(path.as_bytes());
                own.update([0]);
                own.update(identity);
                Some((
                    Unit::Trait {
                        path: path.clone(),
                        trait_id: trait_id.clone(),
                    },
                    own.finalize().into(),
                ))
            })
            .collect::<Vec<_>>();
        let mut named = Vec::new();
        let mut impls = Vec::new();
        for (unit, own) in traits.iter().chain(&self.items) {
            let headed = match (&mut self.kept, &check) {
                (Some(keeping), Some(check)) => {
                    match check.find(unit, *own, |key| headers.digest(key)) {
                        Some(carried) => {
                            keeping.gathered.carried(unit, &carried, check);
                            Arc::clone(carried.value())
                        }
                        None => {
                            self.fresh += 1;
                            let (headed, keys) =
                                written::record(headers.graph(), || write(&headers, unit));
                            let headed = Arc::new(headed?);
                            let keys = keys.into_iter().map(|key| {
                                let digest = headers.digest(&key);
                                (key, digest)
                            });
                            keeping
                                .gathered
                                .fresh(unit.clone(), *own, Arc::clone(&headed), keys);
                            headed
                        }
                    }
                }
                _ => {
                    self.fresh += 1;
                    Arc::new(write(&headers, unit)?)
                }
            };
            match (&*headed, unit) {
                (Headed::Trait(Some(header)), Unit::Trait { path, .. }) => {
                    named.push((path.clone(), header.clone()));
                }
                (Headed::Impls(headers), _) => impls.extend(headers.iter().cloned()),
                _ => {}
            }
        }
        named.sort_by(|left, right| left.0.cmp(&right.0));
        named.dedup_by(|left, right| left.0 == right.0);
        let trait_headers = named.into_iter().filter_map(|(_, header)| header).collect();
        Ok((trait_headers, impls))
    }

    /// How many units this plan wrote, and the units it kept.
    pub(super) fn finish(self) -> (usize, Option<Kept>) {
        (
            self.fresh,
            self.kept.map(|keeping| keeping.gathered.finish()),
        )
    }
}

/// Whether `definition` is a function that gives an opaque type.
fn gives_opaque(definition: &model::Definition) -> bool {
    let model::Kind::Function { signature, .. } = definition.kind() else {
        return false;
    };
    let mut opaques = Vec::new();
    crate::classify::impl_traits(signature.output(), &mut opaques);
    !opaques.is_empty()
}

/// The headers `unit` gives the facade `headers` writes.
fn write<P: api::Phase>(
    headers: &written::Headers<'_, '_, P>,
    unit: &Unit,
) -> Result<Headed, crate::surface::Error> {
    Ok(match unit {
        Unit::Trait { path, trait_id } => Headed::Trait(
            headers
                .trait_header(
                    path,
                    crate::surface::identity::Definition::of(trait_id).as_bytes(),
                )
                .map(trait_header)
                .transpose()?,
        ),
        Unit::Implementation(id) => {
            let implementation = api::reads::paused(|| {
                let graph = headers.graph();
                graph
                    .implementation_index(id)
                    .and_then(|index| graph.implementation_at(index))
            });
            let header =
                implementation.and_then(|implementation| headers.impl_header(implementation));
            Headed::Impls(match header {
                Some(header) => impl_header(header)?.into_iter().collect(),
                None => Box::default(),
            })
        }
        Unit::Opaque(id) => {
            let definition = api::reads::paused(|| {
                let graph = headers.graph();
                graph
                    .definition_index(id)
                    .and_then(|index| graph.definition_at(index))
            });
            let mut converted = Vec::new();
            for header in definition
                .map(|definition| headers.opaque_headers(definition))
                .unwrap_or_default()
            {
                converted.extend(impl_header(header)?);
            }
            Headed::Impls(converted.into_boxed_slice())
        }
    })
}

/// `header` in the facade's types: none when a default can't be written.
fn trait_header(
    header: written::TraitHeader,
) -> Result<Option<TraitHeader>, crate::surface::Error> {
    let Ok(defaults) = header
        .defaults
        .iter()
        .map(|default| {
            default
                .as_ref()
                .map(|default| ty(default, &header.parameters))
                .transpose()
        })
        .collect::<Result<Box<[_]>, _>>()
    else {
        return Ok(None);
    };
    // `Type::Callable` takes its inputs as one: unit, the one input, or
    // their tuple.
    let call = header.call.as_ref().and_then(|(inputs, output)| {
        let input = match &inputs[..] {
            [] => Type::Unit,
            [single] => ty(single, &header.parameters).ok()?,
            many => Type::Tuple(
                many.iter()
                    .map(|input| ty(input, &header.parameters))
                    .collect::<Result<Box<[_]>, _>>()
                    .ok()?,
            ),
        };
        Some(Type::Callable {
            input: Box::new(input),
            output: Box::new(ty(output, &header.parameters).ok()?),
        })
    });
    Ok(Some(TraitHeader {
        path: header.path,
        identity: header.identity,
        parameters: parameters(&header.parameters)?,
        defaults,
        call,
        outputs: None,
        associated: header.associated,
    }))
}

/// `header` in the facade's types: none when its types can't be written.
fn impl_header(header: written::ImplHeader) -> Result<Option<ImplHeader>, crate::surface::Error> {
    let types = |types: &[crate::classify::representability::convert::Type]| {
        types
            .iter()
            .map(|source| ty(source, &header.parameters))
            .collect::<Result<Box<[_]>, _>>()
            .ok()
    };
    let (Some(arguments), Ok(self_type)) = (
        types(&header.arguments),
        ty(&header.self_type, &header.parameters),
    ) else {
        return Ok(None);
    };
    let bounds = header
        .bounds
        .iter()
        .filter_map(|bound| {
            Some(ImplBound {
                ty: ty(&bound.ty, &header.parameters).ok()?,
                trait_identity: bound.trait_identity,
                arguments: types(&bound.arguments)?,
            })
        })
        .collect();
    let projections = header
        .projections
        .iter()
        .filter_map(|projection| {
            Some(ImplProjection {
                ty: ty(&projection.ty, &header.parameters).ok()?,
                value: ty(&projection.value, &header.parameters).ok()?,
                cases: projection
                    .cases
                    .iter()
                    .map(|(this, value)| {
                        Some((
                            ty(this, &header.parameters).ok()?,
                            ty(value, &header.parameters).ok()?,
                        ))
                    })
                    .collect::<Option<_>>()?,
                associated: None,
            })
        })
        .collect();
    Ok(Some(ImplHeader {
        parameters: parameters(&header.parameters)?,
        trait_identity: header.trait_identity,
        arguments,
        self_type,
        bounds,
        projections,
        associated: header
            .associated
            .iter()
            .filter_map(|(name, value)| Some((name.clone(), ty(value, &header.parameters).ok()?)))
            .collect(),
    }))
}
