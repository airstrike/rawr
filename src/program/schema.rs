//! Deterministic nominal schema fingerprints.

use crate::resolve::{item, type_ref};
use crate::{capability, type_def, ImmutableString};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// An exact canonical representation of a nominal type schema.
///
/// Equality is deterministic for a given Rawr schema contract and host binary.
/// The representation is deliberately not a cryptographic digest and does not
/// promise stability across Rawr or Rust versions.
///
/// The parts are shared: cloning a fingerprint never copies its schema, and
/// clones of one fingerprint compare equal by identity.
#[derive(Clone)]
pub struct Fingerprint(crate::Shared<Parts>);

#[derive(Clone)]
struct Parts {
    authority: Box<[Part]>,
    atoms: Box<[Atom]>,
}

impl Fingerprint {
    fn new(authority: Box<[Part]>, atoms: Box<[Atom]>) -> Self {
        Self(crate::Shared::new(Parts { authority, atoms }))
    }
}

/// Every stable nominal identity paired with its exact compiler schema proof.
///
/// This manifest is born only while an executable is assembled from one fully
/// resolved program. Dense resolver identifiers never enter it.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct Manifest(Box<[Definition]>);

/// The exact nominal schemas reachable from one compiler-proven owner.
///
/// An opaque boundary widens the proof to the complete generation manifest;
/// callers never infer the contents of an unaudited host value.
#[derive(Clone, Eq, PartialEq)]
pub struct Reachability {
    definitions: Box<[Definition]>,
    opaque: bool,
}

#[derive(Clone, Eq, PartialEq)]
struct Definition {
    identity: type_def::Identity,
    kind: type_def::Kind,
    schema: Fingerprint,
    capabilities: capability::Fingerprint,
    references: Box<[type_def::Identity]>,
    opaque: bool,
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Fingerprint(<private>)")
    }
}

impl fmt::Debug for Manifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Manifest")
            .field(&format_args!("{} schemas", self.len()))
            .finish()
    }
}

impl fmt::Debug for Reachability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Reachability")
            .field("schemas", &self.len())
            .field("opaque", &self.opaque)
            .finish()
    }
}

impl Fingerprint {
    /// A deterministic private-symbol component for this exact schema.
    ///
    /// This is not a serialized compatibility contract; it only keeps
    /// compiler support for independently compiled ASTs from aliasing when
    /// those ASTs are merged in one process.
    pub(crate) fn symbol_id(&self) -> u64 {
        symbol_id(self)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &crate::host::facade::ProfileFinalization<'_>,
    ) -> usize {
        let parts = crate::Shared::make_mut(&mut self.0);
        debug_assert_eq!(parts.authority.len(), parts.atoms.len());
        parts.authority.iter_mut().zip(parts.atoms.iter()).fold(
            0_usize,
            |completed, (part, atom)| {
                let (Part::Identity(type_def::Identity::Host(identity)), Atom::Host(path)) =
                    (part, atom)
                else {
                    return completed;
                };
                completed
                    .saturating_add(usize::from(profile.normalize_identity_text(path, identity)))
            },
        )
    }
}

impl Manifest {
    pub(crate) fn from_program(program: &crate::resolve::Program) -> Self {
        let evidence = program
            .items()
            .iter()
            .filter_map(|shell| match &shell.kind {
                item::Kind::Struct(item) => Some((
                    item.definition,
                    edges(item.fields.iter().map(|field| &field.ty)),
                )),
                item::Kind::Enum(item) => Some((
                    item.definition,
                    edges(
                        item.variants
                            .iter()
                            .flat_map(|variant| variant.payload.iter()),
                    ),
                )),
                item::Kind::Use(_)
                | item::Kind::Impl(_)
                | item::Kind::Trait(_)
                | item::Kind::Function(_)
                | item::Kind::LegacyFunction(_) => None,
            })
            .collect::<BTreeMap<_, _>>();
        let mut definitions: Vec<_> = program
            .types()
            .iter()
            .map(|definition| {
                let (references, opaque) = match evidence.get(&definition.id()) {
                    Some(evidence) => (
                        evidence.references.clone().into_boxed_slice(),
                        evidence.opaque,
                    ),
                    None => (Box::default(), true),
                };
                Definition {
                    identity: type_def::Identity::Script(definition.key().clone()),
                    kind: definition.kind(),
                    schema: definition.schema().clone(),
                    capabilities: definition.capabilities().fingerprint(),
                    references,
                    opaque,
                }
            })
            .collect();
        definitions.sort_by(|left, right| left.identity.cmp(&right.identity));
        Self(definitions.into_boxed_slice())
    }

    /// Return the number of stable nominal schema proofs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Return whether no nominal schema proof belongs to this program.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate over stable nominal identities and their exact schema proofs.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&type_def::Identity, &Fingerprint)> {
        self.0
            .iter()
            .map(|definition| (&definition.identity, &definition.schema))
    }

    /// Resolve the exact schema proof for one stable nominal identity.
    #[must_use]
    pub fn get(&self, identity: &type_def::Identity) -> Option<&Fingerprint> {
        self.0
            .binary_search_by(|definition| definition.identity.cmp(identity))
            .ok()
            .map(|index| &self.0[index].schema)
    }

    pub(crate) fn reachable(
        &self,
        roots: &BTreeSet<type_def::Identity>,
        opaque_root: bool,
    ) -> Reachability {
        if opaque_root {
            return Reachability {
                definitions: self.0.clone(),
                opaque: true,
            };
        }

        let mut pending = roots.clone();
        let mut visited = BTreeSet::new();
        while let Some(identity) = pending.pop_first() {
            if !visited.insert(identity.clone()) {
                continue;
            }
            let Ok(index) = self
                .0
                .binary_search_by(|definition| definition.identity.cmp(&identity))
            else {
                return Reachability {
                    definitions: self.0.clone(),
                    opaque: true,
                };
            };
            let definition = &self.0[index];
            if definition.opaque {
                return Reachability {
                    definitions: self.0.clone(),
                    opaque: true,
                };
            }
            pending.extend(definition.references.iter().cloned());
        }

        Reachability {
            definitions: self
                .0
                .iter()
                .filter(|definition| visited.contains(&definition.identity))
                .cloned()
                .collect(),
            opaque: false,
        }
    }
}

impl Reachability {
    /// Return the number of exact nominal schemas covered by this proof.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.definitions.len()
    }

    /// Return whether this proof contains no nominal schemas.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// Return whether the proof conservatively covers the complete generation.
    #[must_use]
    pub const fn is_opaque(&self) -> bool {
        self.opaque
    }
}

struct Edges {
    references: Vec<type_def::Identity>,
    opaque: bool,
}

fn edges<'a>(types: impl IntoIterator<Item = &'a type_ref::Type>) -> Edges {
    let mut references = BTreeSet::new();
    let mut opaque = false;
    for ty in types {
        collect_edges(ty, &mut references, &mut opaque);
    }
    Edges {
        references: references.into_iter().collect(),
        opaque,
    }
}

fn collect_edges(
    ty: &type_ref::Type,
    references: &mut BTreeSet<type_def::Identity>,
    opaque: &mut bool,
) {
    match ty {
        type_ref::Type::Unit(_) => {}
        type_ref::Type::Tuple { members, .. } => {
            for member in members {
                collect_edges(member, references, opaque);
            }
        }
        type_ref::Type::Path(path) => {
            match &path.target {
                type_ref::Target::Builtin
                | type_ref::Target::Parameter
                | type_ref::Target::Projection(..) => {}
                type_ref::Target::Host { .. } => *opaque = true,
                type_ref::Target::Script { identity, .. } => {
                    references.insert(identity.clone());
                }
            }
            for argument in &path.arguments {
                if let type_ref::Argument::Type(ty) = argument {
                    collect_edges(ty, references, opaque);
                }
            }
        }
        type_ref::Type::Reference { target, .. } => {
            collect_edges(target, references, opaque);
        }
    }
}

impl PartialEq for Fingerprint {
    fn eq(&self, other: &Self) -> bool {
        crate::Shared::ptr_eq(&self.0, &other.0) || self.0.authority == other.0.authority
    }
}

impl Eq for Fingerprint {}

impl Hash for Fingerprint {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.authority.hash(state);
    }
}

impl PartialOrd for Fingerprint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fingerprint {
    fn cmp(&self, other: &Self) -> Ordering {
        if crate::Shared::ptr_eq(&self.0, &other.0) {
            return Ordering::Equal;
        }
        self.0.authority.cmp(&other.0.authority)
    }
}

/// Hash a canonical proof bundle into a compiler-private symbol component.
pub(crate) fn symbol_id(proof: &impl Hash) -> u64 {
    struct Fnv1a(u64);

    impl Hasher for Fnv1a {
        fn finish(&self) -> u64 {
            self.0
        }

        fn write(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 ^= u64::from(*byte);
                self.0 = self.0.wrapping_mul(0x100_0000_01b3);
            }
        }
    }

    let mut hasher = Fnv1a(0xcbf2_9ce4_8422_2325);
    proof.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Part {
    Marker(Marker),
    Count(usize),
    Text(ImmutableString),
    Identity(type_def::Identity),
}

#[derive(Clone, Debug)]
enum Atom {
    Marker(Marker),
    Count(usize),
    Text(ImmutableString),
    Script(type_def::Key),
    Host(ImmutableString),
    OpaqueHost,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Marker {
    Version1,
    Struct,
    Enum,
    Capabilities,
    Fields,
    Field,
    Variants,
    Variant,
    Unit,
    Tuple,
    Path,
    Builtin,
    Host,
    Script,
    Arguments,
    TypeArgument,
    ErasedLifetime,
    SharedReference,
    MutableReference,
}

struct Encoder {
    parts: Vec<Part>,
    atoms: Vec<Atom>,
}

impl Encoder {
    fn for_type(key: &type_def::Key, kind: Marker) -> Self {
        Self {
            parts: vec![
                Part::Marker(Marker::Version1),
                Part::Marker(kind),
                Part::Identity(type_def::Identity::Script(key.clone())),
            ],
            atoms: vec![
                Atom::Marker(Marker::Version1),
                Atom::Marker(kind),
                Atom::Script(key.clone()),
            ],
        }
    }

    fn finish(self) -> Fingerprint {
        Fingerprint::new(self.parts.into_boxed_slice(), self.atoms.into_boxed_slice())
    }

    fn marker(&mut self, marker: Marker) {
        self.parts.push(Part::Marker(marker));
        self.atoms.push(Atom::Marker(marker));
    }

    fn count(&mut self, count: usize) {
        self.parts.push(Part::Count(count));
        self.atoms.push(Atom::Count(count));
    }

    fn text(&mut self, text: impl Into<ImmutableString>) {
        let text = text.into();
        self.parts.push(Part::Text(text.clone()));
        self.atoms.push(Atom::Text(text));
    }

    fn script(&mut self, identity: type_def::Identity) {
        self.parts.push(Part::Identity(identity.clone()));
        self.atoms.push(match identity {
            type_def::Identity::Script(key) => Atom::Script(key),
            type_def::Identity::Host(_) => Atom::OpaqueHost,
        });
    }

    fn host(&mut self, identity: type_def::Identity, path: impl Into<ImmutableString>) {
        self.parts.push(Part::Identity(identity));
        self.atoms.push(Atom::Host(path.into()));
    }
}

/// The schema of a builtin type as a trait impl owner: its name
/// only, since Rust defines its layout.
pub(crate) fn for_builtin(key: &type_def::Key) -> Fingerprint {
    let mut encoder = Encoder::for_type(key, Marker::Builtin);
    encoder.marker(Marker::Builtin);
    encoder.finish()
}

pub(crate) fn for_struct(item: &item::Struct) -> Fingerprint {
    let mut encoder = Encoder::for_type(&item.key, Marker::Struct);
    write_capabilities(&mut encoder, &item.header);
    encoder.marker(Marker::Fields);
    encoder.count(item.fields.len());

    for field in &item.fields {
        encoder.marker(Marker::Field);
        encoder.text(&field.name.text);
        write_type(&mut encoder, &field.ty);
    }

    encoder.finish()
}

pub(crate) fn for_enum(item: &item::Enum) -> Fingerprint {
    let mut encoder = Encoder::for_type(&item.key, Marker::Enum);
    write_capabilities(&mut encoder, &item.header);
    encoder.marker(Marker::Variants);
    encoder.count(item.variants.len());

    for variant in &item.variants {
        encoder.marker(Marker::Variant);
        encoder.text(&variant.name.text);
        encoder.count(variant.payload.len());

        for ty in &variant.payload {
            write_type(&mut encoder, ty);
        }
    }

    encoder.finish()
}

fn write_capabilities(encoder: &mut Encoder, header: &crate::item::Header) {
    const RELEVANT: &[&str] = &["Clone", "Copy", "Debug", "Default", "Eq", "PartialEq"];

    encoder.marker(Marker::Capabilities);

    let capabilities: Vec<_> = RELEVANT
        .iter()
        .copied()
        .filter(|capability| declares_capability(header, capability))
        .collect();

    encoder.count(capabilities.len());

    for capability in capabilities {
        encoder.text(capability);
    }
}

fn declares_capability(header: &crate::item::Header, capability: &str) -> bool {
    header.attributes.iter().any(|attribute| {
        attribute.path.segments.len() == 1
            && attribute.path.segments[0].name.text == "derive"
            && attribute.arguments.iter().any(|argument| {
                argument.segments.len() == 1
                    && argument.segments[0].name.text.as_str() == capability
            })
    })
}

fn write_type(encoder: &mut Encoder, ty: &type_ref::Type) {
    match ty {
        type_ref::Type::Unit(_) => encoder.marker(Marker::Unit),
        type_ref::Type::Tuple { members, .. } => {
            encoder.marker(Marker::Tuple);
            encoder.count(members.len());
            for member in members {
                write_type(encoder, member);
            }
        }
        type_ref::Type::Path(path) => {
            encoder.marker(Marker::Path);

            match &path.target {
                type_ref::Target::Builtin => {
                    encoder.marker(Marker::Builtin);
                    encoder.text(&path.name);
                }
                type_ref::Target::Parameter | type_ref::Target::Projection(..) => {
                    encoder.marker(Marker::Builtin);
                    encoder.text(format!("<{}>", path.name));
                }
                type_ref::Target::Host { definition, .. } => {
                    encoder.marker(Marker::Host);
                    encoder.host(
                        type_def::Identity::Host(definition.identity().clone()),
                        definition.path(),
                    );
                }
                type_ref::Target::Script { identity, .. } => {
                    encoder.marker(Marker::Script);
                    encoder.script(identity.clone());
                }
            }

            encoder.marker(Marker::Arguments);
            encoder.count(path.arguments.len());

            for argument in &path.arguments {
                match argument {
                    type_ref::Argument::Type(ty) => {
                        encoder.marker(Marker::TypeArgument);
                        write_type(encoder, ty);
                    }
                    type_ref::Argument::Lifetime(_) => {
                        encoder.marker(Marker::ErasedLifetime);
                    }
                }
            }
        }
        type_ref::Type::Reference {
            mutable, target, ..
        } => {
            encoder.marker(if *mutable {
                Marker::MutableReference
            } else {
                Marker::SharedReference
            });
            write_type(encoder, target);
        }
    }
}

impl crate::preview::digest::Write for Fingerprint {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2e00);
        writer.count(self.0.atoms.len());
        for atom in &self.0.atoms {
            match atom {
                Atom::Marker(marker) => writer.tag(schema_tag(*marker)),
                Atom::Count(count) => writer.count(*count),
                Atom::Text(text) => writer.text(text),
                Atom::Script(key) => {
                    writer.tag(0x2e81);
                    key.write(writer)?;
                }
                Atom::Host(path) => {
                    writer.tag(0x2e82);
                    writer.text(path);
                }
                Atom::OpaqueHost => {
                    return Err(crate::preview::digest::Error::OpaqueHostIdentity {
                        context: "preview script schema identity",
                    });
                }
            }
        }
        Ok(())
    }
}

fn schema_tag(marker: Marker) -> u16 {
    match marker {
        Marker::Version1 => 0x2e01,
        Marker::Struct => 0x2e02,
        Marker::Enum => 0x2e03,
        Marker::Capabilities => 0x2e04,
        Marker::Fields => 0x2e05,
        Marker::Field => 0x2e06,
        Marker::Variants => 0x2e07,
        Marker::Variant => 0x2e08,
        Marker::Unit => 0x2e09,
        Marker::Tuple => 0x2e0a,
        Marker::Path => 0x2e0b,
        Marker::Builtin => 0x2e0c,
        Marker::Host => 0x2e0d,
        Marker::Script => 0x2e0e,
        Marker::Arguments => 0x2e0f,
        Marker::TypeArgument => 0x2e10,
        Marker::ErasedLifetime => 0x2e11,
        Marker::SharedReference => 0x2e12,
        Marker::MutableReference => 0x2e13,
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod profile_finalization_tests {
    use super::{Atom, Fingerprint, Part};
    use crate::{host, type_def, ImmutableString};

    const EXACT: [u8; 32] = [0x81; 32];
    const SEMANTIC: [u8; 32] = [0x92; 32];

    fn profile<T: 'static>() -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new("proof", host::facade::Abi::new(1).unwrap()).unwrap();
        builder.compatibility_seal(EXACT).unwrap();
        builder.type_::<T>("Opaque", &[], &[]).unwrap();
        builder.finish().unwrap()
    }

    fn schema(definition: &host::type_def::Definition, ordinary: &str) -> Fingerprint {
        Fingerprint::new(
            Box::from([
                Part::Identity(type_def::Identity::Host(definition.identity().clone())),
                Part::Text(ImmutableString::from(ordinary)),
            ]),
            Box::from([
                Atom::Host(ImmutableString::from("proof::Opaque")),
                Atom::Text(ImmutableString::from(ordinary)),
            ]),
        )
    }

    #[test]
    fn script_schema_finalizes_only_authenticated_host_identity_parts() {
        struct Provisional;
        struct Admitted;

        let provisional = profile::<Provisional>();
        let admitted = profile::<Admitted>();
        let provisional_type = provisional.types().next().unwrap();
        let admitted_type = admitted.types().next().unwrap();
        let provisional_cycle =
            host::facade::ProfileCycle::transform(&provisional, EXACT, SEMANTIC).unwrap();
        let admitted_cycle =
            host::facade::ProfileCycle::transform(&admitted, EXACT, SEMANTIC).unwrap();
        let provisional_finalization = provisional_cycle
            .finalization(provisional.fingerprint())
            .unwrap();
        let admitted_finalization = admitted_cycle.finalization(admitted.fingerprint()).unwrap();
        let mut provisional_schema = schema(provisional_type, "same");
        let mut admitted_schema = schema(admitted_type, "same");

        assert_eq!(
            provisional_schema.normalize_profile(&provisional_finalization),
            1
        );
        assert_eq!(admitted_schema.normalize_profile(&admitted_finalization), 1);
        assert_eq!(provisional_schema, admitted_schema);

        let mut changed = schema(admitted_type, "changed");
        let shared = changed.clone();
        assert!(crate::Shared::ptr_eq(&shared.0, &changed.0));
        assert_eq!(changed.normalize_profile(&admitted_finalization), 1);
        assert_ne!(provisional_schema, changed);
        assert!(!crate::Shared::ptr_eq(&shared.0, &changed.0));
        assert_eq!(shared, schema(admitted_type, "changed"));
        assert_ne!(shared, changed);
    }
}
