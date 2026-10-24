//! Bounded native inventory metadata projected from checked generation inputs.
//!
//! This is not executable authority. A caller must separately own the admitted
//! live generation whose exact carrier supplies this inventory.

use std::fmt;
use std::io::{self, Write};

use serde::ser::SerializeSeq as _;
use serde::{Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::classify::representability::convert;
use crate::surface::{adapter, identity};

const VERSION: u32 = 1;

/// Borrowed compiler facts for one use, without diagnostic source locations.
#[derive(Clone, Copy, Serialize)]
pub struct Use<'a, P = &'a str> {
    /// Canonical imported path, including the imported root.
    pub path: P,
    /// Exact selected operation or structural member.
    pub selector: &'a identity::Selector,
    /// Exact support-only operation site, when this is an endpoint.
    pub endpoint: Option<&'a [u8; 32]>,
    /// Ordered concrete substitutions inferred by executable lowering.
    pub types: &'a [convert::Type],
}

/// Borrowed exact static function operand at one formal position.
#[derive(Clone, Copy, Serialize)]
pub struct Function<'a, P = &'a str> {
    /// Position in the original native signature, including static operands.
    pub position: u32,
    /// Canonical imported function path.
    pub path: P,
    /// Exact imported function selector.
    pub selector: &'a identity::Selector,
}

/// Borrowed script mapper identity at one original formal position.
#[derive(Clone, Copy, Serialize)]
pub struct Mapper<'a> {
    /// Position in the original native signature.
    pub position: u32,
    /// Compiler-derived mapper identity, without its diagnostic range.
    pub seal: &'a [u8; 16],
}

/// One exact metadata match tied to its carrier, not to a live process.
pub struct Matched<'a> {
    carrier: &'a super::Carrier,
    binding: Binding<'a>,
    digest: [u8; 32],
}

/// Exact generated profile endpoint metadata, borrowed from the carrier.
#[derive(Clone, Copy)]
pub struct Endpoint<'a> {
    profile: &'a [u8; 32],
    selector: &'a [u8; 32],
    site: &'a [u8; 32],
    projection: &'a [u8; 32],
    linked: &'a [u8; 32],
}

enum Binding<'a> {
    Adapter(&'a super::Entry),
    Endpoint(Endpoint<'a>),
}

pub(crate) struct Inventory {
    bytes: Box<[u8]>,
    rows: usize,
    digest: [u8; 32],
}

#[derive(Serialize)]
#[serde(bound(serialize = "U: Serialize, Sequence<F>: Serialize, Sequence<M>: Serialize"))]
struct Facts<U, F, M> {
    used: U,
    functions: Sequence<F>,
    mappers: Sequence<M>,
}

struct Sequence<I>(I);

#[derive(Clone, Copy)]
enum Path<'a> {
    Native(&'a adapter::Plan),
    Endpoint(&'a crate::profile::contract::OperationEndpoint),
}

struct Projection<'a> {
    demand: &'a [crate::surface::Entry],
    plans: &'a [adapter::Plan],
    entries: &'a [super::Entry],
    profile: &'a crate::profile::Proof,
}

struct Selected<'a> {
    identity: &'a identity::Identity,
    plan: &'a adapter::Plan,
    functions: &'a [adapter::FunctionItem],
}

struct Measured {
    bytes: usize,
    limit: usize,
    overflow: Option<usize>,
}

struct Comparison<'a> {
    remaining: &'a [u8],
    matched: bool,
}

impl<I> Serialize for Sequence<I>
where
    I: Clone + ExactSizeIterator,
    I::Item: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0.clone() {
            sequence.serialize_element(&item)?;
        }
        sequence.end()
    }
}

impl Serialize for Path<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl fmt::Display for Path<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(plan) => {
                formatter.write_str(plan.export().root_alias())?;
                for segment in plan.export().path() {
                    write!(formatter, "::{segment}")?;
                }
                if matches!(
                    plan.operation(),
                    adapter::Operation::Method { .. }
                        | adapter::Operation::Function {
                            associated: true,
                            ..
                        }
                        | adapter::Operation::Constant {
                            associated: true,
                            ..
                        }
                ) {
                    write!(formatter, "::{}", plan.name())?;
                }
                Ok(())
            }
            Self::Endpoint(endpoint) => {
                write!(
                    formatter,
                    "{}::{}",
                    endpoint.site().root(),
                    endpoint.site().path()
                )
            }
        }
    }
}

impl Inventory {
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn empty() -> Self {
        let mut bytes = Vec::with_capacity(20);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        bytes.extend_from_slice(&0_u64.to_le_bytes());
        Self::retain(bytes.into_boxed_slice(), 0)
    }

    /// Project the resident inventory. `retained_target` is an accounting
    /// target (the generation's JSON target): crossing it is recorded on the
    /// caller's bridge `account` and the projection continues. The
    /// encoded inventory is physically bounded by the IPC diagnostic frame,
    /// `protocol::DIAGNOSTIC_BYTES`, which still refuses.
    pub(crate) fn project<B>(
        request: &mut crate::surface::Request<B>,
        entries: &[super::Entry],
        profile: &crate::profile::Proof,
        retained_target: usize,
        account: &mut crate::surface::account::Account,
    ) -> Result<Self, crate::surface::Error> {
        let projection = Projection {
            demand: &request.entries,
            plans: &request.plans,
            entries,
            profile,
        };
        let (item_limit, _) = request.allocation.limits();
        let mut measured = Measured {
            bytes: 0,
            limit: crate::protocol::DIAGNOSTIC_BYTES,
            overflow: None,
        };
        let result = projection.write(&mut measured);
        if let Some(actual) = measured.overflow {
            return Err(crate::surface::Error::Limit {
                resource: "resident inventory bytes",
                limit: measured.limit,
                actual,
            });
        }
        let rows = result?;
        let retained = measured
            .bytes
            .checked_add(std::mem::size_of::<Self>())
            .ok_or(crate::surface::Error::Allocation)?;
        account.exceeded(
            "resident inventory retained bytes",
            retained_target,
            retained,
        );
        let items = rows.checked_add(1).ok_or(crate::surface::Error::Limit {
            resource: "resident inventory items",
            limit: item_limit,
            actual: usize::MAX,
        })?;
        request.allocation.retain_checked(
            Some(items),
            measured.bytes.checked_add(std::mem::size_of::<Self>()),
        )?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(measured.bytes)
            .map_err(|_| crate::surface::Error::Allocation)?;
        if projection.write(&mut bytes)? != rows || bytes.len() != measured.bytes {
            return Err(crate::surface::Error::Encoding);
        }
        Ok(Self::retain(bytes.into_boxed_slice(), rows))
    }

    /// Decode a carried inventory. The encoded inventory is physically bound
    /// by the IPC diagnostic frame, `protocol::DIAGNOSTIC_BYTES`, and its
    /// counts by checked arithmetic; both refuse. The standard item and JSON
    /// targets are accounting: the producer never refused on them, so a
    /// carrier above them is recorded on a runner account, never rejected.
    pub(crate) fn decode(bytes: &[u8], entries: &[super::Entry]) -> Result<Self, super::Error> {
        if bytes.len() > crate::protocol::DIAGNOSTIC_BYTES {
            return Err(super::Error::Carrier);
        }
        let rows = scan(bytes, entries)?;
        let items = rows
            .checked_add(entries.len())
            .and_then(|n| n.checked_add(1))
            .ok_or(super::Error::Carrier)?;
        let carried = entries
            .len()
            .checked_mul(std::mem::size_of::<super::Entry>())
            .and_then(|size| size.checked_add(std::mem::size_of::<super::Carrier>()))
            .and_then(|size| size.checked_add(bytes.len()))
            .ok_or(super::Error::Carrier)?;
        let limits = crate::cargo::Limits::standard();
        let mut account = crate::surface::account::Account::new(crate::exceedance::Account::Runner);
        account.exceeded("resident inventory carrier items", limits.items, items);
        account.exceeded(
            "resident inventory carrier bytes",
            limits.json_bytes,
            carried,
        );
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(bytes.len())
            .map_err(|_| super::Error::Carrier)?;
        retained.extend_from_slice(bytes);
        Ok(Self::retain(retained.into_boxed_slice(), rows))
    }

    fn retain(bytes: Box<[u8]>, rows: usize) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.native-inventory/v1");
        digest.update(&bytes);
        Self {
            bytes,
            rows,
            digest: digest.finalize().into(),
        }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    pub(crate) fn retained_usage(&self) -> (usize, usize) {
        (
            self.rows + 1,
            std::mem::size_of::<Self>() + self.bytes.len(),
        )
    }

    pub(crate) fn validate(&self, entries: &[super::Entry]) -> Result<(), super::Error> {
        if scan(&self.bytes, entries)? != self.rows {
            return Err(super::Error::Carrier);
        }
        Ok(())
    }
}

impl Projection<'_> {
    fn selected<'a>(&'a self, plan: &'a adapter::Plan) -> Option<Selected<'a>> {
        let identity = self
            .demand
            .iter()
            .map(crate::surface::Entry::identity)
            .find(|identity| identity.as_bytes() == plan.identity().as_bytes())?;
        // The executable collector currently exposes type substitutions and
        // direct static operands. Other instance dimensions cannot be guessed.
        if !identity.constants().is_empty()
            || !identity.binders().is_empty()
            || identity.callable().is_some()
            || !identity.types().iter().all(|ty| concrete(ty, 0))
        {
            return None;
        }
        let (types, functions, mappers) = match plan.operation() {
            adapter::Operation::Function {
                substitutions,
                functions,
                mappers,
                ..
            } => (substitutions.as_ref(), functions.as_ref(), mappers.as_ref()),
            adapter::Operation::Method { substitutions, .. } => {
                (substitutions.as_ref(), &[][..], &[][..])
            }
            adapter::Operation::Builder { .. } => return None,
            _ => (&[][..], &[][..], &[][..]),
        };
        if types != identity.types()
            || functions.len() != identity.function_items().len()
            || mappers.len() != identity.event_mappers().len()
            || functions
                .iter()
                .zip(identity.function_items())
                .any(|(plan, selected)| {
                    plan.position() != selected.position()
                        || plan.plan().identity().as_bytes() != selected.identity().as_bytes()
                })
            || mappers
                .iter()
                .zip(identity.event_mappers())
                .any(|(plan, selected)| {
                    plan.position() != selected.position() || plan.seal() != selected.seal()
                })
        {
            return None;
        }
        Some(Selected {
            identity,
            plan,
            functions,
        })
    }

    fn write(&self, output: &mut impl Write) -> Result<usize, crate::surface::Error> {
        if self.plans.len() != self.entries.len()
            || self
                .plans
                .iter()
                .zip(self.entries)
                .any(|(plan, entry)| plan.identity().as_bytes() != entry.identity())
        {
            return Err(crate::surface::Error::InstanceMismatch);
        }
        output
            .write_all(&VERSION.to_le_bytes())
            .map_err(|_| crate::surface::Error::Encoding)?;
        let native_count = self
            .plans
            .iter()
            .filter(|plan| self.selected(plan).is_some())
            .count();
        length(output, native_count)?;
        for (index, plan) in self.plans.iter().enumerate() {
            let Some(selected) = self.selected(plan) else {
                continue;
            };
            length(output, index)?;
            let mut measured = Measured {
                bytes: 0,
                limit: usize::MAX,
                overflow: None,
            };
            selected.write(&mut measured)?;
            length(output, measured.bytes)?;
            selected.write(output)?;
        }
        let endpoints = self.profile.operation_endpoints();
        length(output, endpoints.len())?;
        for endpoint in endpoints {
            for digest in [
                self.profile.owner_fingerprint(),
                endpoint.selector(),
                endpoint.site().selector(),
                endpoint.projection_selector(),
                endpoint.linked(),
            ] {
                output
                    .write_all(digest)
                    .map_err(|_| crate::surface::Error::Encoding)?;
            }
            let mut measured = Measured {
                bytes: 0,
                limit: usize::MAX,
                overflow: None,
            };
            endpoint_facts(endpoint, &mut measured)?;
            length(output, measured.bytes)?;
            endpoint_facts(endpoint, output)?;
        }
        native_count
            .checked_add(endpoints.len())
            .ok_or(crate::surface::Error::Encoding)
    }
}

impl Selected<'_> {
    fn write(&self, output: &mut impl Write) -> Result<(), crate::surface::Error> {
        let facts = Facts {
            used: Use {
                path: Path::Native(self.plan),
                selector: self.identity.selector(),
                endpoint: None,
                types: self.identity.types(),
            },
            functions: Sequence(
                self.functions
                    .iter()
                    .zip(self.identity.function_items())
                    .map(|(plan, selected)| Function {
                        position: selected.position(),
                        path: Path::Native(plan.plan()),
                        selector: selected.identity().selector(),
                    }),
            ),
            mappers: Sequence(self.identity.event_mappers().iter().map(|mapper| Mapper {
                position: mapper.position(),
                seal: mapper.seal(),
            })),
        };
        serde_json::to_writer(output, &facts).map_err(|_| crate::surface::Error::Encoding)
    }
}

impl<'a> Matched<'a> {
    /// Borrow the exact carrier to which this non-authoritative match belongs.
    #[must_use]
    pub const fn carrier_identity(&self) -> &[u8; 32] {
        self.carrier.identity()
    }

    /// Borrow the exact generated adapter entry, absent for an endpoint.
    #[must_use]
    pub const fn entry(&self) -> Option<&'a super::Entry> {
        match self.binding {
            Binding::Adapter(entry) => Some(entry),
            Binding::Endpoint(_) => None,
        }
    }

    /// Borrow the exact generated profile endpoint lane, absent for an adapter.
    #[must_use]
    pub const fn endpoint(&self) -> Option<Endpoint<'a>> {
        match self.binding {
            Binding::Endpoint(endpoint) => Some(endpoint),
            Binding::Adapter(_) => None,
        }
    }

    /// Source-location-independent matched facts, bound to this exact carrier.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

impl Endpoint<'_> {
    /// Borrow the generated profile-owner fingerprint.
    pub const fn profile(&self) -> &[u8; 32] {
        self.profile
    }
    /// Borrow the profile endpoint selector.
    pub const fn selector(&self) -> &[u8; 32] {
        self.selector
    }
    /// Borrow the authenticated compiler operation-site selector.
    pub const fn site(&self) -> &[u8; 32] {
        self.site
    }
    /// Borrow the required generated projection selector.
    pub const fn projection(&self) -> &[u8; 32] {
        self.projection
    }
    /// Borrow the linked endpoint implementation fingerprint.
    pub const fn linked(&self) -> &[u8; 32] {
        self.linked
    }
}

pub(super) fn cover<'c, 'u, F, M>(
    carrier: &'c super::Carrier,
    used: Use<'u>,
    functions: F,
    mappers: M,
) -> Option<Matched<'c>>
where
    F: ExactSizeIterator<Item = Function<'u>> + Clone,
    M: ExactSizeIterator<Item = Mapper<'u>> + Clone,
{
    let facts = Facts {
        used,
        functions: Sequence(functions),
        mappers: Sequence(mappers),
    };
    if !facts.used.types.iter().all(|ty| concrete(ty, 0)) {
        return None;
    }
    let mut bytes = carrier.coverage.bytes();
    if super::take_u32(&mut bytes).ok()? != VERSION {
        return None;
    }
    let native_count = super::take_length(&mut bytes).ok()?;
    let mut found = None;
    for _ in 0..native_count {
        let index = super::take_length(&mut bytes).ok()?;
        let encoded = fact_bytes(&mut bytes).ok()?;
        if equal(&facts, encoded) {
            if found.is_some() {
                return None;
            }
            found = Some((Binding::Adapter(carrier.entries.get(index)?), encoded));
        }
    }
    let endpoint_count = super::take_length(&mut bytes).ok()?;
    for _ in 0..endpoint_count {
        let endpoint = endpoint(&mut bytes).ok()?;
        let encoded = fact_bytes(&mut bytes).ok()?;
        if equal(&facts, encoded) {
            if found.is_some() {
                return None;
            }
            found = Some((Binding::Endpoint(endpoint), encoded));
        }
    }
    let (binding, encoded) = found?;
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.resident-coverage-match/v1");
    digest.update(carrier.identity());
    digest.update(encoded);
    match &binding {
        Binding::Adapter(entry) => {
            digest.update([0]);
            digest.update(entry.identity());
        }
        Binding::Endpoint(endpoint) => {
            digest.update([1]);
            digest.update(endpoint.selector);
        }
    }
    Some(Matched {
        carrier,
        binding,
        digest: digest.finalize().into(),
    })
}

fn scan(mut bytes: &[u8], entries: &[super::Entry]) -> Result<usize, super::Error> {
    if super::take_u32(&mut bytes)? != VERSION {
        return Err(super::Error::Carrier);
    }
    let count = super::take_length(&mut bytes)?;
    if count > entries.len() {
        return Err(super::Error::Carrier);
    }
    let mut previous = None;
    for _ in 0..count {
        let index = super::take_length(&mut bytes)?;
        if index >= entries.len() || previous.is_some_and(|previous| previous >= index) {
            return Err(super::Error::Carrier);
        }
        previous = Some(index);
        fact_bytes(&mut bytes)?;
    }
    let endpoint_count = super::take_length(&mut bytes)?;
    let rows = count
        .checked_add(endpoint_count)
        .ok_or(super::Error::Carrier)?;
    if rows > crate::cargo::Limits::standard().items {
        return Err(super::Error::Carrier);
    }
    let mut previous = None;
    for _ in 0..endpoint_count {
        let endpoint = endpoint(&mut bytes)?;
        if previous.is_some_and(|previous| previous >= endpoint.selector)
            || [
                endpoint.profile,
                endpoint.selector,
                endpoint.site,
                endpoint.projection,
                endpoint.linked,
            ]
            .iter()
            .any(|digest| **digest == [0; 32])
        {
            return Err(super::Error::Carrier);
        }
        previous = Some(endpoint.selector);
        fact_bytes(&mut bytes)?;
    }
    if !bytes.is_empty() {
        return Err(super::Error::Carrier);
    }
    Ok(rows)
}

fn endpoint<'a>(bytes: &mut &'a [u8]) -> Result<Endpoint<'a>, super::Error> {
    fn digest<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8; 32], super::Error> {
        super::take(bytes, 32)?
            .try_into()
            .map_err(|_| super::Error::Carrier)
    }
    Ok(Endpoint {
        profile: digest(bytes)?,
        selector: digest(bytes)?,
        site: digest(bytes)?,
        projection: digest(bytes)?,
        linked: digest(bytes)?,
    })
}

fn fact_bytes<'a>(bytes: &mut &'a [u8]) -> Result<&'a [u8], super::Error> {
    let length = super::take_length(bytes)?;
    if length == 0 {
        return Err(super::Error::Carrier);
    }
    super::take(bytes, length)
}

fn endpoint_facts(
    endpoint: &crate::profile::contract::OperationEndpoint,
    output: &mut impl Write,
) -> Result<(), crate::surface::Error> {
    let facts = Facts {
        used: Use {
            path: Path::Endpoint(endpoint),
            selector: &identity::Selector::Unavailable,
            endpoint: Some(endpoint.site().selector()),
            types: &[],
        },
        functions: Sequence(std::iter::empty::<Function<'_>>()),
        mappers: Sequence(std::iter::empty::<Mapper<'_>>()),
    };
    serde_json::to_writer(output, &facts).map_err(|_| crate::surface::Error::Encoding)
}

fn equal(facts: &impl Serialize, encoded: &[u8]) -> bool {
    let mut comparison = Comparison {
        remaining: encoded,
        matched: true,
    };
    serde_json::to_writer(&mut comparison, facts).is_ok()
        && comparison.matched
        && comparison.remaining.is_empty()
}

fn concrete(ty: &convert::Type, depth: usize) -> bool {
    if depth >= crate::cargo::Limits::standard().depth {
        return false;
    }
    match ty {
        convert::Type::Parameter(_) | convert::Type::FunctionItem { .. } => false,
        convert::Type::Nominal { arguments, .. } => {
            arguments.iter().all(|ty| concrete(ty, depth + 1))
        }
        convert::Type::ProfileMessage { mapper, payload } => {
            *mapper != [0; 16] && concrete(payload, depth + 1)
        }
        convert::Type::Option(element) | convert::Type::Sequence { element, .. } => {
            concrete(element, depth + 1)
        }
        convert::Type::Result(ok, err) => concrete(ok, depth + 1) && concrete(err, depth + 1),
        convert::Type::Tuple(members) => members.iter().all(|ty| concrete(ty, depth + 1)),
        convert::Type::Unit | convert::Type::Builtin(_) => true,
    }
}

fn length(output: &mut impl Write, value: usize) -> Result<(), crate::surface::Error> {
    let value = u64::try_from(value).map_err(|_| crate::surface::Error::Encoding)?;
    output
        .write_all(&value.to_le_bytes())
        .map_err(|_| crate::surface::Error::Encoding)
}

impl Write for Measured {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let actual = self.bytes.saturating_add(bytes.len());
        if actual > self.limit {
            self.overflow = Some(actual);
            return Err(io::Error::other("resident inventory byte limit"));
        }
        self.bytes = actual;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for Comparison<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.matched {
            match self.remaining.strip_prefix(bytes) {
                Some(remaining) => self.remaining = remaining,
                None => self.matched = false,
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Facts, Function, Inventory, Mapper, Sequence, Use, VERSION};
    use crate::classify::representability::convert;
    use crate::runner::manifest;
    use crate::surface::{self, identity, instance};

    fn snapshot() -> surface::Snapshot {
        let limits = crate::cargo::Limits::standard();
        let mut snapshot = surface::Snapshot::builder(&limits);
        snapshot.entry("/fixture/main.rs").unwrap();
        snapshot.file("/fixture/main.rs", b"fixture").unwrap();
        snapshot.seal().unwrap()
    }

    fn entries(request: &surface::Request<()>) -> Box<[manifest::Entry]> {
        request
            .plans
            .iter()
            .map(|plan| crate::bridge::generate::entry(plan).unwrap())
            .collect()
    }

    fn static_contract() -> crate::Contract<crate::Classified> {
        use crate::api::{self, ledger, model};

        // Reuse the existing complete fixture graph, giving its constant and
        // unsafe function a static consumer and a second compatible callback.
        let graph = crate::test_fixture::graph();
        let target = |name| {
            graph
                .exports()
                .find_map(|export| match export {
                    ledger::Path::Imported(export) if export.key().path().eq([name]) => {
                        Some(export.target().clone())
                    }
                    _ => None,
                })
                .unwrap()
        };
        let callback = target("item_3");
        let alternate = target("item_4");
        let consumer = target("item_2");
        let model::Kind::Function {
            signature,
            generics,
        } = graph
            .definitions()
            .find(|definition| definition.id() == &callback)
            .unwrap()
            .kind()
        else {
            panic!("existing fixture callback is a function");
        };
        let definitions = graph
            .definitions()
            .map(|definition| {
                let kind = if definition.id() == &consumer {
                    model::Kind::Function {
                        signature: model::Signature::new(
                            model::Safety::Safe,
                            false,
                            false,
                            "\"Rust\"".into(),
                            false,
                            Box::default(),
                            model::Receiver::None,
                            Box::from([model::Type::FunctionPointer(Box::new(signature.clone()))]),
                            signature.output().clone(),
                        ),
                        generics: generics.clone(),
                    }
                } else if definition.id() == &alternate {
                    model::Kind::Function {
                        signature: signature.clone(),
                        generics: generics.clone(),
                    }
                } else {
                    return definition.clone();
                };
                model::Definition::new(
                    definition.id().clone(),
                    definition.display_path().map(Box::<str>::from).collect(),
                    definition.owner().clone(),
                    definition.visibility().clone(),
                    kind,
                    definition.span().clone(),
                    definition.origin().clone(),
                )
            })
            .collect();
        let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
        api::Graph::seal(
            &api::Context::fixture(),
            Vec::new(),
            definitions,
            graph.exports().cloned().collect(),
            graph.implementations().cloned().collect(),
            graph.scalar_predicate_proofs().cloned().collect(),
            graph.support().cloned().collect(),
            graph.anonymous_imports().cloned().collect(),
            usize::MAX,
            &mut allocation,
        )
        .unwrap()
        .publish(crate::input::proof::Proof::fixture())
        .unwrap()
        .classify(
            &crate::classify::Policy::standard(),
            &crate::cargo::Limits::standard(),
        )
        .unwrap()
    }

    fn projected_carrier(mut request: surface::Request<()>) -> manifest::Carrier {
        use sha2::Digest as _;

        let entries = entries(&request);
        let inventory = Inventory::project(
            &mut request,
            &entries,
            &crate::profile::Proof::neutral(),
            crate::cargo::Limits::standard().json_bytes,
            &mut crate::surface::account::Account::bridge(),
        )
        .unwrap();
        let generated = manifest::GeneratedManifest::with_coverage(
            crate::bridge::BuildRequestKey::fixture([3; 32]),
            Box::default(),
            entries,
            inventory,
            Box::default(),
            sha2::Sha256::digest([]).into(),
        );
        generated.validate(&[]).unwrap();
        manifest::Carrier::transform(&generated.carrier().unwrap()).unwrap()
    }

    #[test]
    fn native_inventory_projects_checked_nominal_declarations() {
        let limits = crate::cargo::Limits::standard();
        let contract = static_contract();
        let structure = contract
            .outcomes()
            .find(|outcome| outcome.export().path().eq(["item_0"]))
            .unwrap();
        let mut compiler = contract.compiler((), snapshot(), &limits).unwrap();
        compiler
            .require_selected(
                structure.identity(),
                identity::Selector::Declaration,
                "/fixture/main.rs",
                0,
                1,
            )
            .unwrap();
        let request = compiler.seal().unwrap().into_request();
        assert!(matches!(
            request.plans[0].operation(),
            surface::adapter::Operation::Declaration { .. }
        ));
        let carrier = projected_carrier(request);
        let used = Use {
            path: "fixture::item_0",
            selector: &identity::Selector::Declaration,
            endpoint: None,
            types: &[],
        };
        let matched = carrier
            .cover(used, std::iter::empty(), std::iter::empty())
            .unwrap();
        assert!(matched.entry().is_some());
        assert!(carrier
            .cover(
                Use {
                    path: "fixture::item_1",
                    ..used
                },
                std::iter::empty(),
                std::iter::empty(),
            )
            .is_none());
    }

    #[test]
    fn native_inventory_projects_checked_static_operands_without_rawr_ranges() {
        let limits = crate::cargo::Limits::standard();
        let contract = static_contract();
        let outcome = |name| {
            contract
                .outcomes()
                .find(|outcome| outcome.export().path().eq([name]))
                .unwrap()
        };
        let consumer = outcome("item_2");
        let callback = outcome("item_3");
        let alternate = outcome("item_4");
        let compile = |start, selected: &identity::Identity| {
            let function = instance::FunctionItem::selected(
                0,
                selected,
                selected.selector().clone(),
                start + 1,
                start + 2,
            )
            .unwrap();
            let arguments = instance::Arguments::new(
                Box::default(),
                Box::default(),
                Box::default(),
                None,
                &limits,
            )
            .unwrap()
            .with_function_items(Box::from([function]), &limits)
            .unwrap();
            let mut compiler = contract.compiler((), snapshot(), &limits).unwrap();
            compiler
                .require_instance(
                    consumer.identity(),
                    consumer.identity().selector().clone(),
                    arguments,
                    "/fixture/main.rs",
                    start,
                    start + 4,
                )
                .unwrap();
            compiler.seal().unwrap().into_request()
        };
        let original = compile(0, callback.identity());
        let moved = compile(100, callback.identity());
        assert_ne!(
            original.entries[0].identity().as_bytes(),
            moved.entries[0].identity().as_bytes()
        );
        let original_operand = original.entries[0].identity().function_items()[0].byte_range();
        let moved_operand = moved.entries[0].identity().function_items()[0].byte_range();
        assert_ne!(original_operand, moved_operand);
        let carrier = projected_carrier(original);
        let used = Use {
            path: "fixture::item_2",
            selector: consumer.identity().selector(),
            endpoint: None,
            types: &[],
        };
        let moved_functions = moved.entries[0]
            .identity()
            .function_items()
            .iter()
            .map(|function| Function {
                position: function.position(),
                path: "fixture::item_3",
                selector: function.identity().selector(),
            });
        let matched = carrier
            .cover(used, moved_functions.clone(), std::iter::empty())
            .unwrap();
        assert!(matched.entry().is_some());
        let digest = *matched.digest();
        assert_eq!(
            *carrier
                .cover(used, moved_functions, std::iter::empty())
                .unwrap()
                .digest(),
            digest
        );

        // The alternate callback is independently accepted by the same real
        // classifier and static-operand checker, but is absent from this native generation.
        let changed = compile(100, alternate.identity());
        let changed_functions =
            changed.entries[0]
                .identity()
                .function_items()
                .iter()
                .map(|function| Function {
                    position: function.position(),
                    path: "fixture::item_4",
                    selector: function.identity().selector(),
                });
        assert!(carrier
            .cover(used, changed_functions, std::iter::empty())
            .is_none());
    }

    #[test]
    fn native_inventory_projects_moved_checked_generic_calls_and_rejects_changed_types() {
        let limits = crate::cargo::Limits::standard();
        let contract = crate::test_fixture::representability_families()
            .classify(&crate::classify::Policy::standard(), &limits)
            .unwrap();
        let selected = contract
            .outcomes()
            .find(|outcome| outcome.export().path().eq(["family_2"]))
            .unwrap();
        let compile = |start| {
            let mut compiler = contract.compiler((), snapshot(), &limits).unwrap();
            let arguments = instance::Arguments::new(
                Box::from([convert::Type::Builtin(convert::Builtin::I64)]),
                Box::default(),
                Box::default(),
                None,
                &limits,
            )
            .unwrap();
            compiler
                .require_instance(
                    selected.identity(),
                    selected.identity().selector().clone(),
                    arguments,
                    "/fixture/main.rs",
                    start,
                    start + 1,
                )
                .unwrap();
            compiler.seal().unwrap().into_request()
        };
        let mut original = compile(0);
        let mut moved = compile(20);
        assert_ne!(original.canonical, moved.canonical);
        let rows = entries(&original);
        let moved_rows = entries(&moved);
        let before = original.allocation.retained_usage();
        let coverage = Inventory::project(
            &mut original,
            &rows,
            &crate::profile::Proof::neutral(),
            limits.json_bytes,
            &mut crate::surface::account::Account::bridge(),
        )
        .unwrap();
        let after = original.allocation.retained_usage();
        assert_eq!(after.0 - before.0, coverage.retained_usage().0);
        assert_eq!(after.1 - before.1, coverage.retained_usage().1);
        // The retained target is accounting. Exactly the retained size
        // admits silently; one byte less records a truthful high water on the
        // bridge account and the projection still completes identically.
        let mut exact = compile(0);
        let mut exact_account = crate::surface::account::Account::bridge();
        let exact_coverage = Inventory::project(
            &mut exact,
            &rows,
            &crate::profile::Proof::neutral(),
            coverage.retained_usage().1,
            &mut exact_account,
        )
        .unwrap();
        assert_eq!(exact_coverage.bytes(), coverage.bytes());
        assert!(!exact_account.exceedances().is_exceeded());
        let mut short = compile(0);
        let before_short = short.allocation.retained_usage();
        let mut short_account = crate::surface::account::Account::bridge();
        let short_coverage = Inventory::project(
            &mut short,
            &rows,
            &crate::profile::Proof::neutral(),
            coverage.retained_usage().1 - 1,
            &mut short_account,
        )
        .unwrap();
        assert_eq!(short_coverage.bytes(), coverage.bytes());
        let axis = *short_account
            .exceedances()
            .axis("resident inventory retained bytes")
            .unwrap();
        assert_eq!(axis.target as usize, coverage.retained_usage().1 - 1);
        assert_eq!(axis.high_water as usize, coverage.retained_usage().1);
        assert_eq!(
            short.allocation.retained_usage().1 - before_short.1,
            coverage.retained_usage().1
        );
        let relocated = Inventory::project(
            &mut moved,
            &moved_rows,
            &crate::profile::Proof::neutral(),
            limits.json_bytes,
            &mut crate::surface::account::Account::bridge(),
        )
        .unwrap();
        assert_eq!(coverage.bytes(), relocated.bytes());
        let coverage = Inventory::decode(coverage.bytes(), &rows).unwrap();
        let carrier = manifest::Carrier {
            identity: [4; 32],
            entries: rows,
            coverage,
        };
        let types = [convert::Type::Builtin(convert::Builtin::I64)];
        let used = Use {
            path: "fixture::family_2",
            selector: selected.identity().selector(),
            endpoint: None,
            types: &types,
        };
        let matched = carrier
            .cover(used, std::iter::empty(), std::iter::empty())
            .unwrap();
        assert_eq!(
            matched.entry().unwrap().identity(),
            carrier.entries[0].identity()
        );
        let changed = [convert::Type::Builtin(convert::Builtin::Bool)];
        assert!(carrier
            .cover(
                Use {
                    types: &changed,
                    ..used
                },
                std::iter::empty(),
                std::iter::empty()
            )
            .is_none());
        let unresolved = [convert::Type::Parameter(0)];
        assert!(carrier
            .cover(
                Use {
                    types: &unresolved,
                    ..used
                },
                std::iter::empty(),
                std::iter::empty()
            )
            .is_none());
        assert!(carrier
            .cover(
                Use {
                    path: "other::family_2",
                    ..used
                },
                std::iter::empty(),
                std::iter::empty()
            )
            .is_none());
    }

    fn encoded_carrier(facts: &impl serde::Serialize) -> manifest::Carrier {
        let facts = serde_json::to_vec(facts).unwrap();
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&VERSION.to_le_bytes());
        super::length(&mut encoded, 1).unwrap();
        super::length(&mut encoded, 0).unwrap();
        super::length(&mut encoded, facts.len()).unwrap();
        encoded.extend_from_slice(&facts);
        super::length(&mut encoded, 0).unwrap();
        let entries = Box::from([manifest::Entry::new([5; 32], [7; 32], 1)]);
        let coverage = Inventory::decode(&encoded, &entries).unwrap();
        manifest::Carrier {
            identity: [4; 32],
            entries,
            coverage,
        }
    }

    #[test]
    fn native_inventory_static_operands_and_mappers_are_exact_and_carrier_bound() {
        let selector = identity::Selector::Function {
            item: identity::Definition::authenticated([1; 32]),
        };
        let function_selector = identity::Selector::Function {
            item: identity::Definition::authenticated([2; 32]),
        };
        let functions = [Function {
            position: 1,
            path: "fixture::map",
            selector: &function_selector,
        }];
        let mappers = [Mapper {
            position: 2,
            seal: &[3; 16],
        }];
        let types = [convert::Type::ProfileMessage {
            mapper: [3; 16],
            payload: Box::new(convert::Type::Builtin(convert::Builtin::I64)),
        }];
        let used = Use {
            path: "fixture::call",
            selector: &selector,
            endpoint: None,
            types: &types,
        };
        let facts = Facts {
            used,
            functions: Sequence(functions.iter().copied()),
            mappers: Sequence(mappers.iter().copied()),
        };
        let carrier = encoded_carrier(&facts);
        let first = carrier
            .cover(used, functions.iter().copied(), mappers.iter().copied())
            .unwrap();
        let first_digest = *first.digest();
        let changed_path = [Function {
            path: "fixture::different",
            ..functions[0]
        }];
        assert!(carrier
            .cover(used, changed_path.iter().copied(), mappers.iter().copied())
            .is_none());
        let changed_selector = [Function {
            selector: &selector,
            ..functions[0]
        }];
        assert!(carrier
            .cover(
                used,
                changed_selector.iter().copied(),
                mappers.iter().copied()
            )
            .is_none());
        let changed_position = [Function {
            position: 0,
            ..functions[0]
        }];
        assert!(carrier
            .cover(
                used,
                changed_position.iter().copied(),
                mappers.iter().copied()
            )
            .is_none());
        let changed_mapper = [Mapper {
            seal: &[9; 16],
            ..mappers[0]
        }];
        assert!(carrier
            .cover(
                used,
                functions.iter().copied(),
                changed_mapper.iter().copied()
            )
            .is_none());
        assert!(carrier
            .cover(used, functions.iter().copied(), std::iter::empty())
            .is_none());
        let mut other_carrier = encoded_carrier(&facts);
        other_carrier.identity = [9; 32];
        assert_ne!(
            *other_carrier
                .cover(used, functions.iter().copied(), mappers.iter().copied())
                .unwrap()
                .digest(),
            first_digest
        );
    }

    #[test]
    fn native_inventory_rejects_truncated_unknown_version_and_foreign_entries() {
        let selector = identity::Selector::Declaration;
        let facts = Facts {
            used: Use {
                path: "fixture::Type",
                selector: &selector,
                endpoint: None,
                types: &[],
            },
            functions: Sequence(std::iter::empty::<Function<'_>>()),
            mappers: Sequence(std::iter::empty::<Mapper<'_>>()),
        };
        let carrier = encoded_carrier(&facts);
        let bytes = carrier.coverage.bytes();
        for length in 0..bytes.len() {
            assert!(Inventory::decode(&bytes[..length], &carrier.entries).is_err());
        }
        assert!(Inventory::decode(bytes, &[]).is_err());
        let mut stale = bytes.to_vec();
        stale[..4].copy_from_slice(&0_u32.to_le_bytes());
        assert!(Inventory::decode(&stale, &carrier.entries).is_err());
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(Inventory::decode(&trailing, &carrier.entries).is_err());
    }
}
