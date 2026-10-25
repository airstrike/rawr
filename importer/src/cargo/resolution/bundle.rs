//! Persistable association of historical resolution data and its complete files.
//! This layer authenticates bytes; current Cargo/input replay is still required.

use super::layout::{Layout, Paths};
use crate::input::stored::{self, Budget, Commitment, Record};
use crate::node;
use crate::runner::artifact::retained;
use crate::store::{self, bundle};

const MAGIC: &[u8; 8] = b"RAWRRES1";
const SCHEMA: u32 = 3;
const BYTES: usize = 156;

/// Historical bundle commitments, without a current node receipt.
pub(crate) struct Stored {
    reference: bundle::BundleRef,
    seal_bytes: usize,
    seal_digest: [u8; 32],
    record: Commitment,
}

/// Unpublished ownership stays alive until the semantic owner validates it.
pub(crate) struct Sealed {
    stored: Stored,
    pending: bundle::Pending,
    #[cfg(test)]
    graph: node::GraphBudget,
}

/// Authenticated historical bytes with their exact read-only bundle geometry.
pub(crate) struct Observed<'budget, 'account> {
    paths: Paths,
    bytes: Vec<u8>,
    budget: &'budget mut Budget<'account>,
}

/// Private replay continuation. Geometry and historical controls are authenticated;
/// content validation must finish before a semantic replay result can escape.
pub(crate) struct PendingReplay<'store, 'budget, 'account> {
    observed: Observed<'budget, 'account>,
    inventory: Inventory<'store>,
}

pub(crate) struct Inventory<'store> {
    stored: retained::Stored,
    origin: retained::Origin<'store>,
    scope: node::read_set::Scope,
    graph: node::GraphBudget,
    completion: Option<bundle::Completion<'store>>,
}

impl<'store, 'budget, 'account> PendingReplay<'store, 'budget, 'account> {
    pub(crate) fn paths(&self) -> &Paths {
        self.observed.paths()
    }

    pub(crate) fn require_bytes(
        &self,
        path: &std::path::Path,
        bytes: &[u8],
    ) -> Result<(), node::Error> {
        self.inventory.require_bytes(
            self.paths().root(),
            path,
            bytes,
            self.observed.budget.graph(),
        )
    }

    /// The sealed commitment for one early control file of this observation.
    pub(crate) fn commitment(
        &self,
        path: &std::path::Path,
    ) -> Result<(u64, [u8; 32]), node::Error> {
        self.inventory.commitment(self.paths().root(), path)
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        Paths,
        Vec<u8>,
        Inventory<'store>,
        &'budget mut Budget<'account>,
    ) {
        (
            self.observed.paths,
            self.observed.bytes,
            self.inventory,
            self.observed.budget,
        )
    }

    #[cfg(test)]
    fn complete_inventory(self) -> stored::Result<Observed<'budget, 'account>> {
        self.inventory
            .replay(self.observed.paths.root(), self.observed.budget.graph())
            .map_err(|error| self.observed.budget.failure(stored::Error::Bundle(error)))?;
        Ok(self.observed)
    }
}

impl Inventory<'_> {
    pub(crate) fn commitment(
        &self,
        root: &std::path::Path,
        path: &std::path::Path,
    ) -> Result<(u64, [u8; 32]), node::Error> {
        self.stored.resolution_commitment(root, path)
    }

    pub(crate) fn require_bytes(
        &self,
        root: &std::path::Path,
        path: &std::path::Path,
        bytes: &[u8],
        graph: &node::GraphBudget,
    ) -> Result<(), node::Error> {
        if !self.graph.shares_with(graph) {
            return Err(node::Error::Codec("inventory graph differs"));
        }
        self.stored
            .require_resolution_bytes(root, self.origin, path, bytes, graph)
    }

    fn replay(self, root: &std::path::Path, graph: &node::GraphBudget) -> Result<(), node::Error> {
        if !self.graph.shares_with(graph) {
            return Err(node::Error::Codec("inventory graph differs"));
        }
        retained::observe_hashes(graph, |hashes| {
            self.stored.replay_resolution_content_completing(
                root,
                self.origin,
                graph,
                node::read_set::Scope::Complete,
                self.completion,
                |path, metadata, _| hashes.hash(path, metadata, bundle::MAX_BYTES as usize),
            )
        })
    }

    pub(crate) fn complete(
        self,
        root: &std::path::Path,
        namespace: Option<&crate::extract::plan::namespace::Retained>,
        allocation: &mut crate::extract::Allocation,
        graph: &node::GraphBudget,
        limit: usize,
    ) -> crate::Result<()> {
        if !self.graph.shares_with(graph) {
            return Err(crate::Error::Node {
                operation: "complete resolution inventory",
                source: Box::new(node::Error::Codec("inventory graph differs")),
            });
        }
        match namespace {
            Some(namespace) => namespace
                .revalidate_inventory_completing(
                    &self.stored,
                    root,
                    self.origin,
                    self.scope,
                    self.completion,
                    allocation,
                    graph,
                    limit,
                )
                .map(|_| ()),
            None => self
                .replay(root, graph)
                .map_err(|source| crate::Error::Node {
                    operation: "complete resolution inventory",
                    source: Box::new(source),
                }),
        }
    }
}

impl<'budget, 'account> Observed<'budget, 'account> {
    pub(crate) fn paths(&self) -> &Paths {
        &self.paths
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Stored {
    pub(crate) fn bundle_commitment(&self) -> bundle::BundleCommitment {
        bundle::BundleCommitment::new(self.reference, self.seal_bytes, self.seal_digest)
    }

    /// Persist the record and transfer the existing complete inventory directly
    /// into the seal, without cloning it or publishing any current authority.
    pub(crate) fn capture(
        layout: Layout,
        record: &Record<'_>,
        scope: &super::content::Scope,
        budget: &mut Budget<'_>,
    ) -> stored::Result<Sealed> {
        let result = (|| {
            budget.check()?;
            if !layout.budget().shares_with(budget.graph()) {
                return Err(stored::Error::Encoding(
                    "resolution bundle graph account differs",
                ));
            }
            let record = record.persist(&layout, budget)?;
            let graph = budget.graph().clone();
            let reference = layout.reference();
            let inventory = retained::Stored::capture_resolution(
                layout.root(),
                reference,
                retained::Mode::Pending,
                &graph,
                scope,
            )
            .map_err(stored::Error::Bundle)?;
            let stored = Self {
                reference,
                seal_bytes: inventory.bytes().len(),
                seal_digest: inventory.digest(),
                record,
            };
            let pending = layout
                .seal(inventory.into_bytes())
                .map_err(stored::Error::Bundle)?;
            let sealed = Sealed {
                stored,
                pending,
                #[cfg(test)]
                graph,
            };
            // This returns unpublished historical ownership only. The owner
            // reads and validates it immediately before semantic replay; doing
            // that here too discards an entire duplicate inventory traversal.
            Ok(sealed)
        })();
        result.map_err(|error| budget.failure(error))
    }

    pub(crate) fn encode(&self) -> [u8; BYTES] {
        let mut bytes = [0; BYTES];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&SCHEMA.to_le_bytes());
        bytes[12..44].copy_from_slice(&self.reference.execution().as_bytes());
        bytes[44..76].copy_from_slice(&self.reference.id().as_bytes());
        bytes[76..84].copy_from_slice(&(self.seal_bytes as u64).to_le_bytes());
        bytes[84..116].copy_from_slice(&self.seal_digest);
        bytes[116..].copy_from_slice(&self.record.encode());
        bytes
    }

    pub(crate) fn decode(bytes: &[u8], budget: &mut Budget<'_>) -> stored::Result<Self> {
        let result = (|| {
            budget.check()?;
            budget
                .graph()
                .decode(bytes.len())
                .map_err(stored::Error::Graph)?;
            if bytes.len() != BYTES
                || &bytes[..8] != MAGIC
                || part::<4>(bytes, 8)? != SCHEMA.to_le_bytes()
            {
                return Err(stored::Error::Encoding("resolution bundle header changed"));
            }
            let seal_bytes = usize::try_from(u64::from_le_bytes(part(bytes, 76)?))
                .map_err(|_| stored::Error::Overflow)?;
            if seal_bytes == 0 || seal_bytes > node::MAX_CONTENT_BYTES {
                return Err(stored::Error::Encoding(
                    "resolution bundle seal extent is invalid",
                ));
            }
            Ok(Self {
                reference: bundle::BundleRef::new(
                    node::ExecutionKey::from_bytes(part(bytes, 12)?),
                    bundle::BundleId::from_bytes(part(bytes, 44)?),
                ),
                seal_bytes,
                seal_digest: part(bytes, 84)?,
                record: Commitment::decode(&part(bytes, 116)?)?,
            })
        })();
        result.map_err(|error| budget.failure(error))
    }

    /// Validate the actual published controls and complete frozen file set before
    /// exposing historical input bytes. This does not interpret those inputs.
    #[cfg(test)]
    pub(crate) fn read_published<'budget, 'account>(
        &self,
        store: &store::Store,
        kind: node::Kind,
        schema: u32,
        key: node::Digest,
        budget: &'budget mut Budget<'account>,
    ) -> stored::Result<Observed<'budget, 'account>> {
        self.read_published_for_replay(store, kind, schema, key, budget)?
            .complete_inventory()
    }

    #[cfg(test)]
    pub(crate) fn read_published_for_replay<'store, 'budget, 'account>(
        &self,
        store: &'store store::Store,
        kind: node::Kind,
        schema: u32,
        key: node::Digest,
        budget: &'budget mut Budget<'account>,
    ) -> stored::Result<PendingReplay<'store, 'budget, 'account>> {
        self.read_published_scoped_for_replay(
            store,
            kind,
            schema,
            key,
            budget,
            node::read_set::Scope::Complete,
        )
    }

    pub(crate) fn read_published_scoped_for_replay<'store, 'budget, 'account>(
        &self,
        store: &'store store::Store,
        kind: node::Kind,
        schema: u32,
        key: node::Digest,
        budget: &'budget mut Budget<'account>,
        scope: node::read_set::Scope,
    ) -> stored::Result<PendingReplay<'store, 'budget, 'account>> {
        let result = (|| {
            budget.check()?;
            let (root, completion) = if scope == node::read_set::Scope::Complete {
                let (root, completion) = bundle::Completion::authenticate(
                    store.root(),
                    self.bundle_commitment(),
                    kind,
                    schema,
                    key,
                    budget.graph(),
                )
                .map_err(stored::Error::Bundle)?;
                (root, Some(completion))
            } else {
                (
                    bundle::validate_consumer_commitment(
                        store.root(),
                        &self.bundle_commitment(),
                        kind,
                        schema,
                        key,
                        budget.graph(),
                    )
                    .map_err(stored::Error::Bundle)?,
                    None,
                )
            };
            self.observe(
                root,
                retained::Origin::Published(store, kind, schema, key),
                scope,
                completion,
                budget,
            )
        })();
        match result {
            Ok((paths, bytes, inventory)) => Ok(PendingReplay {
                observed: Observed {
                    paths,
                    bytes,
                    budget,
                },
                inventory,
            }),
            Err(error) => Err(budget.failure(error)),
        }
    }

    fn observe<'store>(
        &self,
        root: std::path::PathBuf,
        origin: retained::Origin<'store>,
        scope: node::read_set::Scope,
        completion: Option<bundle::Completion<'store>>,
        budget: &mut Budget<'_>,
    ) -> stored::Result<(Paths, Vec<u8>, Inventory<'store>)> {
        let graph = budget.graph().clone();
        let seal_path = bundle::path(&root, std::path::Path::new("seal"), &graph)
            .map_err(stored::Error::Bundle)?;
        let bytes = store::read_file_budgeted(&seal_path, node::MAX_CONTENT_BYTES, &graph)
            .map_err(stored::Error::Bundle)?;
        let inventory = retained::Stored::decode_resolution(&bytes, &root, self.reference, &graph)
            .map_err(stored::Error::Bundle)?;
        if bytes.len() != self.seal_bytes || inventory.digest() != self.seal_digest {
            return Err(stored::Error::Encoding(
                "resolution bundle inventory changed",
            ));
        }
        inventory
            .resolution_controls_scoped(&root, origin, &graph, node::read_set::Scope::Consumer)
            .map_err(stored::Error::Bundle)?;
        let paths = Paths::observe(root, &graph).map_err(stored::Error::Bundle)?;
        let bytes = self.record.read(&paths, budget)?;
        Ok((
            paths,
            bytes,
            Inventory {
                stored: inventory,
                origin,
                scope,
                graph,
                completion,
            },
        ))
    }
}

impl Sealed {
    #[cfg(test)]
    pub(crate) fn read<'budget, 'account>(
        &self,
        budget: &'budget mut Budget<'account>,
    ) -> stored::Result<Observed<'budget, 'account>> {
        self.read_for_replay(budget)?.complete_inventory()
    }

    /// The sealed, unpublished bundle as a replay reads it; production replays
    /// a resolution only after publication.
    #[cfg(test)]
    pub(crate) fn read_for_replay<'budget, 'account>(
        &self,
        budget: &'budget mut Budget<'account>,
    ) -> stored::Result<PendingReplay<'static, 'budget, 'account>> {
        let result = (|| {
            budget.check()?;
            if !self.graph.shares_with(budget.graph()) {
                return Err(stored::Error::Encoding(
                    "pending resolution graph account differs",
                ));
            }
            let (root, completion) = self
                .pending
                .complete_replay(self.stored.bundle_commitment(), budget.graph())
                .map_err(stored::Error::Bundle)?;
            self.stored.observe(
                root,
                retained::Origin::Unpublished(retained::Mode::Pending),
                node::read_set::Scope::Complete,
                Some(completion),
                budget,
            )
        })();
        match result {
            Ok((paths, bytes, inventory)) => Ok(PendingReplay {
                observed: Observed {
                    paths,
                    bytes,
                    budget,
                },
                inventory,
            }),
            Err(error) => Err(budget.failure(error)),
        }
    }

    pub(crate) fn into_parts(self) -> (Stored, bundle::Pending) {
        (self.stored, self.pending)
    }
}

fn part<const N: usize>(bytes: &[u8], offset: usize) -> stored::Result<[u8; N]> {
    bytes
        .get(offset..offset + N)
        .and_then(|part| part.try_into().ok())
        .ok_or(stored::Error::Encoding("resolution bundle field truncated"))
}
