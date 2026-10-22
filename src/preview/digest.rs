//! Versioned canonical digest evidence for one exact private preview proof.

use std::fmt;

use crate::{item, receiver, type_def};

use super::{Callback, Input, Key, LogicalSize, Proof, Seed, Theme};

const VERSION: &[u8] = b"rawr-preview-proof-digest-v1";
const SUBJECT_VERSION: &[u8] = b"rawr-preview-subject-digest-v1";
const LEFT_OFFSET: u64 = 0xcbf29ce484222325;
const RIGHT_OFFSET: u64 = 0x6c62272e07bb0142;
const PRIME: u64 = 0x100000001b3;

/// Opaque, versioned evidence derived from one exact compiler-private preview proof.
///
/// A digest is useful in manifests and diagnostics. It is not compiler
/// authority and must never replace exact [`super::Fingerprint`] equality.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Digest([u8; 16]);

/// A compiler invariant required by the canonical digest was not present.
#[derive(Debug)]
pub enum Error {
    /// A source-owned slot retained only an opaque Rust representation.
    OpaqueHostIdentity {
        /// Exact proof slot missing semantic identity.
        context: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpaqueHostIdentity { context } => write!(
                formatter,
                "preview digest invariant failed: {context} retained a host TypeId without a stable semantic path"
            ),
        }
    }
}

#[cfg(not(feature = "no_std"))]
impl std::error::Error for Error {}

#[cfg(feature = "no_std")]
impl core_error::Error for Error {}

pub(super) fn of(proof: &Proof) -> Result<Digest, Error> {
    let mut writer = Writer::new();
    proof.write(&mut writer)?;
    Ok(writer.finish())
}

/// Return canonical digest evidence for one stable preview subject identity.
///
/// Preview variants on the same subject intentionally share this evidence.
/// The exact variant remains part of [`super::Key`] and the snapshot manifest.
pub fn subject(key: &item::Key) -> Result<Digest, Error> {
    let mut writer = Writer::with_version(SUBJECT_VERSION);
    key.write(&mut writer)?;
    Ok(writer.finish())
}

/// Return non-authoritative, domain-separated identity for one exact symbolic
/// event mapper. Runtime use must still rebind the matching [`item::Value`]
/// through the current generation authority.
pub(crate) fn event_mapper(value: &item::Value) -> Result<[u8; 16], Error> {
    let mut writer = Writer::with_version(b"rawr-profile-event-mapper-v1");
    value.write(&mut writer)?;
    Ok(writer.finish().bytes())
}

impl Digest {
    /// Return the fixed-width canonical digest bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Digest({self})")
    }
}

/// Crate-private canonical atoms accepted by preview digest participants.
///
/// This is intentionally not a serializer and does not implement
/// [`std::hash::Hasher`]. Every participant must choose its semantic tags and
/// fields explicitly.
pub(crate) struct Writer {
    left: u64,
    right: u64,
}

pub(crate) trait Write {
    fn write(&self, writer: &mut Writer) -> Result<(), Error>;
}

impl Writer {
    fn new() -> Self {
        Self::with_version(VERSION)
    }

    pub(crate) fn with_version(version: &[u8]) -> Self {
        let mut writer = Self {
            left: LEFT_OFFSET,
            right: RIGHT_OFFSET,
        };
        writer.frame(0, version);
        writer
    }

    /// Write a semantic structure, field, or enum-variant tag.
    pub(crate) fn tag(&mut self, tag: u16) {
        self.frame(1, &tag.to_be_bytes());
    }

    /// Write a platform-independent collection cardinality.
    pub(crate) fn count(&mut self, count: usize) {
        self.frame(2, &(count as u64).to_be_bytes());
    }

    /// Write a UTF-8 semantic name or path.
    pub(crate) fn text(&mut self, text: &str) {
        self.frame(3, text.as_bytes());
    }

    /// Write a fixed-width unsigned integer.
    pub(crate) fn u32(&mut self, value: u32) {
        self.frame(4, &value.to_be_bytes());
    }

    /// Write a boolean under an explicit one-byte representation.
    pub(crate) fn bool(&mut self, value: bool) {
        self.frame(5, &[u8::from(value)]);
    }

    /// Write opaque canonical bytes already framed by their owning protocol.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.frame(6, bytes);
    }

    fn frame(&mut self, atom: u8, bytes: &[u8]) {
        self.absorb(&[atom]);
        self.absorb(&(bytes.len() as u64).to_be_bytes());
        self.absorb(bytes);
    }

    fn absorb(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.left ^= u64::from(*byte);
            self.left = self.left.wrapping_mul(PRIME);
            self.right ^= u64::from(*byte);
            self.right = self.right.rotate_left(5).wrapping_mul(PRIME);
        }
    }

    pub(crate) fn finish(self) -> Digest {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&self.left.to_be_bytes());
        bytes[8..].copy_from_slice(&self.right.to_be_bytes());
        Digest(bytes)
    }
}

impl Write for Proof {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1000);
        writer.tag(0x1001);
        self.key.write(writer)?;
        writer.tag(0x1002);
        writer.text(&self.name);
        writer.tag(0x1003);
        writer.text(&self.group);
        writer.tag(0x1004);
        self.subject.write(writer)?;
        writer.tag(0x1005);
        self.input.write(writer)?;
        writer.tag(0x1006);
        self.message.write(writer)?;
        writer.tag(0x1007);
        self.seed.write(writer)?;
        writer.tag(0x1008);
        write_option(writer, self.update.as_ref())?;
        writer.tag(0x1009);
        write_slice(writer, &self.themes)?;
        writer.tag(0x100a);
        writer.bool(self.snapshot);
        writer.tag(0x100b);
        self.size.write(writer)?;
        writer.tag(0x100c);
        self.runtime_output.write(writer)?;
        writer.tag(0x100d);
        write_option(writer, self.projection.as_ref())?;
        Ok(())
    }
}

impl Write for Key {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1100);
        writer.tag(0x1101);
        self.subject.write(writer)?;
        writer.tag(0x1102);
        writer.text(&self.variant);
        Ok(())
    }
}

impl Write for LogicalSize {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1200);
        writer.tag(0x1201);
        writer.u32(self.width);
        writer.tag(0x1202);
        writer.u32(self.height);
        Ok(())
    }
}

impl Write for Input {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        match self {
            Self::Stateless => writer.tag(0x1301),
            Self::Shared(ty) => {
                writer.tag(0x1302);
                ty.write(writer)?;
            }
            Self::Owned(ty) => {
                writer.tag(0x1303);
                ty.write(writer)?;
            }
        }
        Ok(())
    }
}

impl Write for Callback {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1400);
        writer.tag(0x1401);
        self.item.write(writer)?;
        writer.tag(0x1402);
        write_receiver(writer, self.receiver);
        writer.tag(0x1403);
        write_slice(writer, &self.parameters)?;
        writer.tag(0x1404);
        self.output.write(writer)?;
        Ok(())
    }
}

impl Write for Seed {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        match self {
            Self::None => writer.tag(0x1501),
            Self::Default => writer.tag(0x1502),
            Self::Callback(callback) => {
                writer.tag(0x1503);
                callback.write(writer)?;
            }
        }
        Ok(())
    }
}

impl Write for Theme {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1600);
        writer.tag(0x1601);
        writer.text(&self.name);
        writer.tag(0x1602);
        self.callback.write(writer)?;
        Ok(())
    }
}

impl Write for item::Key {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1700);
        writer.tag(0x1701);
        writer.text(self.package());
        writer.tag(0x1702);
        writer.count(self.modules().len());
        for module in self.modules() {
            writer.text(module);
        }
        writer.tag(0x1703);
        write_script_option(writer, self.owner(), "preview item key owner")?;
        writer.tag(0x1704);
        write_item_kind(writer, self.kind());
        writer.tag(0x1705);
        writer.text(self.name());
        Ok(())
    }
}

impl Write for item::Value {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1800);
        writer.tag(0x1801);
        self.key().write(writer)?;
        writer.tag(0x1802);
        write_item_kind(writer, self.kind());
        writer.tag(0x1803);
        write_script_option(writer, self.owner(), "preview callback owner")?;
        writer.tag(0x1804);
        write_receiver(writer, self.receiver());
        writer.tag(0x1805);
        self.signature().write(writer)?;
        writer.tag(0x1806);
        self.capabilities().write(writer)?;
        Ok(())
    }
}

impl Write for type_def::Key {
    fn write(&self, writer: &mut Writer) -> Result<(), Error> {
        writer.tag(0x1900);
        writer.tag(0x1901);
        writer.text(self.package());
        writer.tag(0x1902);
        writer.count(self.modules().len());
        for module in self.modules() {
            writer.text(module);
        }
        writer.tag(0x1903);
        writer.text(self.name());
        Ok(())
    }
}

fn write_item_kind(writer: &mut Writer, kind: item::Kind) {
    writer.tag(match kind {
        item::Kind::Struct => 0x1b01,
        item::Kind::Enum => 0x1b02,
        item::Kind::Function => 0x1b03,
        item::Kind::AssociatedFunction => 0x1b04,
        item::Kind::Method => 0x1b05,
        item::Kind::VariantConstructor => 0x1b06,
    });
}

pub(crate) fn write_receiver(writer: &mut Writer, receiver: Option<receiver::Mode>) {
    writer.tag(match receiver {
        None => 0x1c00,
        Some(receiver::Mode::Owned) => 0x1c01,
        Some(receiver::Mode::Shared) => 0x1c02,
        Some(receiver::Mode::Mutable) => 0x1c03,
    });
}

pub(crate) fn write_slice<T: Write>(writer: &mut Writer, values: &[T]) -> Result<(), Error> {
    writer.count(values.len());
    for value in values {
        value.write(writer)?;
    }
    Ok(())
}

pub(crate) fn write_option<T: Write>(writer: &mut Writer, value: Option<&T>) -> Result<(), Error> {
    match value {
        Some(value) => {
            writer.tag(0x1d01);
            value.write(writer)?;
        }
        None => writer.tag(0x1d00),
    }
    Ok(())
}

pub(crate) fn write_script_identity(
    writer: &mut Writer,
    identity: &type_def::Identity,
    context: &'static str,
) -> Result<(), Error> {
    let type_def::Identity::Script(key) = identity else {
        return Err(Error::OpaqueHostIdentity { context });
    };
    writer.tag(0x1a01);
    key.write(writer)
}

fn write_script_option(
    writer: &mut Writer,
    identity: Option<&type_def::Identity>,
    context: &'static str,
) -> Result<(), Error> {
    match identity {
        Some(identity) => {
            writer.tag(0x1d01);
            write_script_identity(writer, identity, context)
        }
        None => {
            writer.tag(0x1d00);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use crate::{
        application, capability, diagnostic, host, item, program, signature, source, type_def, Span,
    };

    use super::Writer;
    use crate::preview::{Definition, Locations};

    use super::{Callback, Input, Key, LogicalSize, Seed};

    #[test]
    fn canonical_atoms_have_a_fixed_golden_vector() {
        let mut writer = Writer::new();
        writer.tag(0xabcd);
        writer.text("preview");
        writer.count(7);
        writer.u32(42);
        writer.bool(true);
        assert_eq!(
            writer.finish().to_string(),
            "b26c98c813dff1c31981bbbc0ca9c06f"
        );
    }

    #[test]
    fn atom_tags_lengths_and_order_perturb_the_digest() {
        fn digest(write: impl FnOnce(&mut Writer)) -> super::Digest {
            let mut writer = Writer::new();
            write(&mut writer);
            writer.finish()
        }

        assert_ne!(
            digest(|writer| writer.tag(1)),
            digest(|writer| writer.tag(2))
        );
        assert_ne!(
            digest(|writer| writer.text("a")),
            digest(|writer| writer.text("a\0"))
        );
        assert_ne!(
            digest(|writer| {
                writer.tag(1);
                writer.tag(2);
            }),
            digest(|writer| {
                writer.tag(2);
                writer.tag(1);
            })
        );
    }

    #[test]
    fn exact_definition_has_a_fixed_golden_vector() {
        let mut definition = definition(host::runtime::type_ref::Type::unit(), None);

        assert_eq!(
            definition.digest().unwrap().to_string(),
            "4877332b22e0ab644878db5779575b92"
        );
        assert_eq!(
            super::subject(definition.key.subject())
                .unwrap()
                .to_string(),
            "b86b6f4babf0a089826adf32f0dfeb2c"
        );
        let original = super::subject(definition.key.subject()).unwrap();
        definition.key.variant = "wide".into();
        assert_eq!(original, super::subject(definition.key.subject()).unwrap());
    }

    #[test]
    fn subject_identity_fields_each_perturb_digest_evidence() {
        let plain = item::Key::new(
            "app".into(),
            vec!["controls".into()].into_boxed_slice(),
            "view".into(),
            item::Kind::Function,
        );
        let package = item::Key::new(
            "dependency".into(),
            vec!["controls".into()].into_boxed_slice(),
            "view".into(),
            item::Kind::Function,
        );
        let module = item::Key::new(
            "app".into(),
            vec!["widgets".into()].into_boxed_slice(),
            "view".into(),
            item::Kind::Function,
        );
        let name = item::Key::new(
            "app".into(),
            vec!["controls".into()].into_boxed_slice(),
            "preview".into(),
            item::Kind::Function,
        );
        let kind = item::Key::new(
            "app".into(),
            vec!["controls".into()].into_boxed_slice(),
            "view".into(),
            item::Kind::AssociatedFunction,
        );
        let owner = type_def::Key::new(
            "app".into(),
            vec!["controls".into()].into_boxed_slice(),
            "Counter".into(),
        );
        let other_owner = type_def::Key::new(
            "app".into(),
            vec!["controls".into()].into_boxed_slice(),
            "Other".into(),
        );
        let associated = item::Key::associated(&owner, "view".into(), item::Kind::Method);
        let other_associated =
            item::Key::associated(&other_owner, "view".into(), item::Kind::Method);
        let original = super::subject(&plain).unwrap();

        for changed in [&package, &module, &name, &kind, &associated] {
            assert_ne!(original, super::subject(changed).unwrap());
        }
        assert_ne!(
            super::subject(&associated).unwrap(),
            super::subject(&other_associated).unwrap()
        );
    }

    #[test]
    fn runtime_output_and_projection_each_perturb_definition_evidence() {
        let base = definition(host::runtime::type_ref::Type::unit(), None);
        let runtime = definition(
            host::runtime::type_ref::Type::builtin("i64", TypeId::of::<i64>()),
            None,
        );
        let projection = definition(host::runtime::type_ref::Type::unit(), Some(projection()));

        assert_ne!(base.digest().unwrap(), runtime.digest().unwrap());
        assert_ne!(base.digest().unwrap(), projection.digest().unwrap());
    }

    #[test]
    fn representation_type_id_does_not_enter_path_bearing_evidence() {
        let left = definition(
            host::runtime::type_ref::Type::host(
                "test::Value".into(),
                TypeId::of::<u32>(),
                Box::default(),
                None,
            ),
            None,
        );
        let right = definition(
            host::runtime::type_ref::Type::host(
                "test::Value".into(),
                TypeId::of::<u64>(),
                Box::default(),
                None,
            ),
            None,
        );
        let renamed = definition(
            host::runtime::type_ref::Type::host(
                "test::Other".into(),
                TypeId::of::<u32>(),
                Box::default(),
                None,
            ),
            None,
        );

        assert_eq!(left.digest().unwrap(), right.digest().unwrap());
        assert_ne!(left.digest().unwrap(), renamed.digest().unwrap());
    }

    #[test]
    fn bare_host_owner_fails_before_digest_evidence_exists() {
        let mut definition = definition(host::runtime::type_ref::Type::unit(), None);
        definition.subject.item = item::Value::new(
            definition.subject.item.key().clone(),
            item::Kind::Function,
            Some(crate::type_def::Identity::Host(
                host::type_def::Identity::linked(TypeId::of::<u32>()),
            )),
            None,
            signature::Fingerprint::function(false, None, &[], None),
            capability::Set::new().fingerprint(),
        );

        assert!(matches!(
            definition.digest(),
            Err(super::Error::OpaqueHostIdentity {
                context: "preview callback owner"
            })
        ));
    }

    fn definition(
        runtime_output: host::runtime::type_ref::Type,
        projection: Option<host::projection::Site>,
    ) -> Definition {
        let key = item::Key::new(
            "digest".into(),
            Box::default(),
            "view".into(),
            item::Kind::Function,
        );
        let value = item::Value::new(
            key.clone(),
            item::Kind::Function,
            None,
            None,
            signature::Fingerprint::function(false, None, &[], None),
            capability::Set::new().fingerprint(),
        );
        let unit = application::Type::digest_unit();
        let callback = Callback {
            item: value,
            receiver: None,
            parameters: Box::default(),
            output: unit.clone(),
        };
        let source = program::source::Source::from_resolved(
            source::Map::new(""),
            diagnostic::Source::new("digest.rawr", "").unwrap(),
        );
        let location = super::super::Location {
            source: source.key(program::source::Id::ROOT),
            span: Span::NONE,
        };

        Definition {
            key: Key {
                subject: key,
                variant: "default".into(),
            },
            name: "Default".into(),
            group: "digest".into(),
            subject: callback,
            input: Input::Stateless,
            message: unit,
            seed: Seed::None,
            update: None,
            themes: Box::default(),
            snapshot: true,
            size: LogicalSize {
                width: 320,
                height: 240,
            },
            runtime_output,
            projection,
            locations: Locations {
                attribute: location,
                subject: location,
                input: None,
                message: location,
                seed: None,
                update: None,
                themes: Box::default(),
            },
        }
    }

    fn projection() -> host::projection::Site {
        struct Source;
        struct Target;

        let abi = host::facade::Abi::new(1).unwrap();
        let mut builder = host::facade::Builder::new("digest", abi).unwrap();
        builder.type_::<Source>("Source", &[], &[]).unwrap();
        builder.type_::<Target>("Target", &[], &[]).unwrap();
        builder.project("Source", "Target", &[]).unwrap();
        let facade = builder.finish().unwrap();
        let source = facade
            .types()
            .find(|definition| definition.path() == "digest::Source")
            .unwrap();
        let target = facade
            .types()
            .find(|definition| definition.path() == "digest::Target")
            .unwrap();
        let owner = host::facade::Dependency::new(facade.root_path(), facade.fingerprint().clone());
        let edge = source.projections()[0].clone();
        let source_runtime = host::runtime::type_ref::Type::host(
            source.path().into(),
            source
                .native_identity()
                .expect("test facade source type is linked"),
            Box::default(),
            Some(owner.clone()),
        );
        let target_runtime = host::runtime::type_ref::Type::host(
            target.path().into(),
            target
                .native_identity()
                .expect("test facade target type is linked"),
            Box::default(),
            Some(owner.clone()),
        );
        host::projection::Site::new(
            host::projection::Key::new(owner, source.contract(), edge),
            source_runtime,
            target_runtime,
            Span::NONE,
        )
    }
}
