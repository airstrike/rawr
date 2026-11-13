//! Owned API capture awaiting the connected source-consumer union.

use crate::node;

/// Build the captured graph from what `Basis` names, only when asked.
pub(crate) type Materialize =
    Box<dyn FnOnce(Basis<'_>) -> crate::Result<super::Graph<super::Captured>> + Send>;

/// What a captured graph is built from.
pub(crate) enum Basis<'a> {
    /// The compiler sidecars the capture read.
    Sidecars,
    /// The stored API product, its parts read back.
    Stored(&'a super::cached::Product),
    /// The graph an owner kept of the same semantic content under another
    /// build, which this build's context and invocations frame; the stored
    /// `product` when they don't.
    Kept {
        graph: &'a super::Graph<super::Captured>,
        product: &'a super::cached::Product,
    },
}

/// The captured graph, or the deferred rebuild of a current stored API hit.
// Ready is the usual result and is matched at once; boxing it would allocate per hit.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Lazy {
    Ready(super::Graph<super::Captured>),
    Pending(Materialize),
}

/// Authenticated API data without a current input or parent capability.
///
/// A current API hit defers its graph: a resolved warm launch never builds it.
pub struct Continuation {
    pub(crate) product: node::staging::Owned<super::cached::Product>,
    pub(crate) graph: Lazy,
    pub(crate) publication: Publication,
}

/// A continuation whose graph has been built for classification.
pub(crate) struct Materialized {
    pub(crate) product: node::staging::Owned<super::cached::Product>,
    pub(crate) graph: super::Graph<super::Captured>,
    pub(crate) publication: Publication,
}

pub(crate) struct Publication {
    pub(crate) context: crate::context::Context,
    pub(crate) retention: node::read_set::Retention,
    source: Source,
}

/// What the API was read from: a complete resolution owner and its compared
/// inputs, or shared bindings replayed under the toolchain's attestation
/// with the proof of the files that run read (`crate::bindings`).
// One publication per run, matched at once; boxing would only add an allocation.
#[allow(clippy::large_enum_variant)]
enum Source {
    Owner {
        owner: node::Validated<crate::cargo::resolution::owner::Product>,
        input: crate::input::stored::Completed,
    },
    #[cfg(feature = "rustc-driver")]
    Shared {
        parent: std::sync::Arc<node::Validated<crate::cargo::toolchain::Attestation>>,
        proof: crate::input::proof::Proof,
    },
}

impl Continuation {
    /// The identity of every input this API was read from: the resolution
    /// owner and its compared files, or the shared replay's input proof. A
    /// dependency's facts can change only when this does.
    #[must_use]
    pub fn input_identity(&self) -> [u8; 32] {
        use sha2::{Digest as _, Sha256};
        let mut hash = Sha256::new();
        match &self.publication.source {
            Source::Owner { owner, input } => {
                hash.update(b"owner");
                hash.update(owner.content().digest().as_bytes());
                hash.update(input.fingerprint().as_bytes());
            }
            #[cfg(feature = "rustc-driver")]
            Source::Shared { proof, .. } => {
                hash.update(b"shared");
                hash.update(proof.fingerprint().as_bytes());
            }
        }
        hash.finalize().into()
    }

    pub(crate) fn capture(
        context: &crate::context::Context,
        owner: node::Validated<crate::cargo::resolution::owner::Product>,
        input: crate::input::stored::Completed,
        product: node::staging::Owned<super::cached::Product>,
        retention: node::read_set::Retention,
        graph: Lazy,
    ) -> Result<Self, node::Error> {
        retention.covers_parents(std::slice::from_ref(&product))?;
        input
            .require_account(context.graph_budget())
            .map_err(|error| node::Error::Transform(error.to_string().into()))?;
        Ok(Self {
            product,
            graph,
            publication: Publication {
                context: context.clone(),
                retention,
                source: Source::Owner { owner, input },
            },
        })
    }

    /// The API of a run that replayed shared bindings, under the toolchain
    /// attestation it was keyed by.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn capture_shared(
        context: &crate::context::Context,
        parent: std::sync::Arc<node::Validated<crate::cargo::toolchain::Attestation>>,
        proof: crate::input::proof::Proof,
        product: node::staging::Owned<super::cached::Product>,
        retention: node::read_set::Retention,
        graph: Lazy,
    ) -> Result<Self, node::Error> {
        retention.covers_parents(std::slice::from_ref(&product))?;
        Ok(Self {
            product,
            graph,
            publication: Publication {
                context: context.clone(),
                retention,
                source: Source::Shared { parent, proof },
            },
        })
    }

    /// Build the captured graph, from the stored API product when deferred.
    /// What it reads of the API counts where it is read (#1786).
    pub(crate) fn materialize(self) -> Result<Materialized, node::Error> {
        let graph = build(&self.product, self.graph)?;
        Ok(Materialized {
            product: self.product,
            graph,
            publication: self.publication,
        })
    }

    /// Publish the native-input proof of a current API without its graph.
    /// The owner already revalidated this evidence; the checks match
    /// `publish`. The graph comes back unbuilt, for an owner that keeps
    /// graphs to build off this thread ([`Kept::keep`]); anyone else drops
    /// it.
    pub(crate) fn into_native_proof(
        self,
    ) -> Result<(crate::input::proof::Proof, Deferred), node::Error> {
        let Self {
            product,
            graph,
            publication,
        } = self;
        let node::staging::Owned::Current(api) = &product else {
            return Err(node::Error::State("historical API has no current proof"));
        };
        let proof = publication.proof(api)?;
        Ok((proof, Deferred { product, graph }))
    }
}

/// The graph of an API a warm launch resolved without it: built only by an
/// owner that keeps it for its next save ([`Kept::keep`]).
pub struct Deferred {
    product: node::staging::Owned<super::cached::Product>,
    graph: Lazy,
}

/// What [`Kept`] holds for the API product its key names.
// One per owner, matched at once; boxing the graph would only add an allocation.
#[allow(clippy::large_enum_variant)]
enum Held {
    Built(super::Graph<super::Captured>),
    Building(std::thread::JoinHandle<Result<super::Graph<super::Captured>, node::Error>>),
}

/// The API graph a long-running `rawr run` owner keeps between saves: the
/// one it built last, or the one a warm start builds in the background,
/// beside the identity of the API product it is the graph of. Each save
/// that changes only Rawr source needs the same graph, and decoding it from
/// the store takes seconds; kept, a save shares it and copies nothing.
#[derive(Default)]
pub struct Kept {
    held: Option<(Key, Held)>,
    /// The classification of the kept graph's API the last save replayed.
    classification: crate::classify::kept::Kept,
}

/// An API product's action and content.
type Key = (node::ActionKey, node::Digest);

impl Kept {
    /// The classification this owner keeps beside its graph.
    pub(crate) fn classification(&mut self) -> &mut crate::classify::kept::Kept {
        &mut self.classification
    }

    /// Build the graph a warm launch deferred off this thread, so the next
    /// save that changes Rawr source finds it built. One already kept or
    /// building for an API of the same content is not built again: a save
    /// under this build frames it ([`Kept::graph`]).
    pub fn keep(&mut self, deferred: Deferred) {
        let Deferred { product, graph } = deferred;
        let key = product.identity();
        if self.held.as_ref().is_some_and(|(held, _)| held.1 == key.1) {
            return;
        }
        let held = match graph {
            Lazy::Ready(graph) => Held::Built(graph),
            Lazy::Pending(materialize) => Held::Building(std::thread::spawn(move || {
                let began = std::time::Instant::now();
                let graph = crate::progress::uncounted(|| rebuild(&product, materialize));
                crate::progress::deferred_graph(began.elapsed());
                graph
            })),
        };
        self.held = Some((key, held));
    }

    /// `api` with its graph built: the one kept for its API product, the
    /// one a background build of it is finishing, or one built now. The
    /// graph is kept for the next save. A background build of another API
    /// is not waited for.
    ///
    /// A new build whose API has the content of the kept one, as after a
    /// native edit that changes no crate's API, frames the kept graph with
    /// its own context and invocations: it decodes and seals nothing.
    pub fn graph(&mut self, mut api: Continuation) -> Result<Continuation, node::Error> {
        let key = api.product.identity();
        let held = match self.held.take() {
            Some((held, graph)) if held.1 == key.1 => Some((held == key, graph)),
            _ => None,
        };
        let graph = match (api.graph, held) {
            (Lazy::Ready(graph), _) => graph,
            (Lazy::Pending(materialize), Some((exact, held))) => {
                let graph = match held {
                    Held::Built(graph) => Some(graph),
                    Held::Building(building) => {
                        let began = std::time::Instant::now();
                        let built = building.join();
                        crate::progress::waited_for_graph(began.elapsed());
                        // A background build that failed or panicked is
                        // built here, which reports the failure as its own.
                        built.ok().and_then(Result::ok)
                    }
                };
                match graph {
                    Some(graph) if exact => graph,
                    Some(graph) => frame(&api.product, materialize, &graph)?,
                    None => rebuild(&api.product, materialize)?,
                }
            }
            (Lazy::Pending(materialize), None) => rebuild(&api.product, materialize)?,
        };
        api.graph = Lazy::Ready(graph.share());
        self.held = Some((key, Held::Built(graph)));
        Ok(api)
    }
}

/// The graph of `product` under its own build, framed from `kept`, a graph
/// of the same content under another build (#1786).
fn frame(
    product: &node::staging::Owned<super::cached::Product>,
    materialize: Materialize,
    kept: &super::Graph<super::Captured>,
) -> Result<super::Graph<super::Captured>, node::Error> {
    Ok(materialize(Basis::Kept {
        graph: kept,
        product: product.value(),
    })?)
}

/// The graph of `product`: the captured one, or one rebuilt from the
/// stored product.
fn build(
    product: &node::staging::Owned<super::cached::Product>,
    graph: Lazy,
) -> Result<super::Graph<super::Captured>, node::Error> {
    match graph {
        Lazy::Ready(graph) => Ok(graph),
        Lazy::Pending(materialize) => rebuild(product, materialize),
    }
}

/// The graph of `product`, decoded from the stored product.
fn rebuild(
    product: &node::staging::Owned<super::cached::Product>,
    materialize: Materialize,
) -> Result<super::Graph<super::Captured>, node::Error> {
    let began = std::time::Instant::now();
    let product = product.value();
    let resolved = product.resolve()?;
    let graph = materialize(Basis::Stored(resolved.as_ref().unwrap_or(product)))?;
    crate::progress::rebuilt_graph(began.elapsed());
    Ok(graph)
}

impl Publication {
    pub(crate) fn callback_observations(&self, graph: &super::Graph<super::Captured>) -> [u8; 96] {
        match &self.source {
            Source::Owner { input, .. } => graph.callback_observations(input.fingerprint()),
            #[cfg(feature = "rustc-driver")]
            Source::Shared { proof, .. } => graph.callback_observations(proof.fingerprint()),
        }
    }

    fn check(&self, api: &node::Validated<super::cached::Product>) -> Result<(), node::Error> {
        self.retention
            .covers_parents(std::slice::from_ref(&node::Parent::validated(api)))?;
        let parent = match &self.source {
            Source::Owner { owner, .. } => owner.receipt().read_set(),
            #[cfg(feature = "rustc-driver")]
            Source::Shared { parent, .. } => parent.receipt().read_set(),
        };
        api.receipt().read_set().require_ancestor(parent)
    }

    fn proof(
        self,
        api: &node::Validated<super::cached::Product>,
    ) -> Result<crate::input::proof::Proof, node::Error> {
        self.check(api)?;
        match self.source {
            Source::Owner { owner, input } => {
                Ok(input.publish(&owner, self.context.graph_budget())?)
            }
            #[cfg(feature = "rustc-driver")]
            Source::Shared { proof, .. } => Ok(proof),
        }
    }

    pub(crate) fn publish(
        self,
        graph: super::Graph<super::Captured>,
        api: node::Validated<super::cached::Product>,
    ) -> Result<super::Graph<super::Extracted>, node::Error> {
        self.check(&api)?;
        let input = match self.source {
            Source::Owner { owner, input } => input.publish(&owner, self.context.graph_budget())?,
            #[cfg(feature = "rustc-driver")]
            Source::Shared { proof, .. } => proof,
        };
        let chain = node::Chain::validated_retained(self.context, &api, self.retention)?;
        Ok(graph.publish_with_chain(input, Some(chain))?)
    }
}
