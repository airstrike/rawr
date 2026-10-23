//! Canonical fingerprints for resolved callable signatures.

#[cfg(feature = "rust-import")]
use crate::host;
use crate::resolve::{item, type_ref};
use crate::{receiver, type_def, ImmutableString};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// An exact canonical callable-signature fingerprint.
///
/// Equality compares the structural representation directly. This is not a
/// digest, a wire format, or a cross-version compatibility promise. The parts
/// are shared: cloning never copies them, and clones compare equal by identity.
#[derive(Clone)]
pub struct Fingerprint(crate::Shared<Parts>);

#[derive(Clone)]
struct Parts {
    authority: Box<[Part]>,
    atoms: Box<[Atom]>,
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
    Free,
    Associated,
    Owned,
    Shared,
    Mutable,
    Parameters,
    Output,
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
    fn new() -> Self {
        Self {
            parts: vec![Part::Marker(Marker::Version1)],
            atoms: vec![Atom::Marker(Marker::Version1)],
        }
    }

    fn finish(self) -> Fingerprint {
        Fingerprint(crate::Shared::new(Parts {
            authority: self.parts.into_boxed_slice(),
            atoms: self.atoms.into_boxed_slice(),
        }))
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

impl fmt::Debug for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Fingerprint(<private>)")
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

impl Fingerprint {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        let parts = crate::Shared::make_mut(&mut self.0);
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

    pub(crate) fn function(
        associated: bool,
        receiver: Option<receiver::Mode>,
        parameters: &[item::Parameter],
        output: Option<&type_ref::Type>,
    ) -> Self {
        let mut encoder = Encoder::new();
        encoder.marker(match receiver {
            Some(receiver::Mode::Owned) => Marker::Owned,
            Some(receiver::Mode::Shared) => Marker::Shared,
            Some(receiver::Mode::Mutable) => Marker::Mutable,
            None if associated => Marker::Associated,
            None => Marker::Free,
        });
        encoder.marker(Marker::Parameters);
        encoder.count(parameters.len());
        for parameter in parameters {
            write_type(&mut encoder, &parameter.ty);
        }
        encoder.marker(Marker::Output);
        if let Some(output) = output {
            write_type(&mut encoder, output);
        } else {
            encoder.marker(Marker::Unit);
        }
        encoder.finish()
    }

    /// A lifted closure: its arity and its rendered signature.
    pub(crate) fn closure(arity: usize, signature: &str) -> Self {
        let mut encoder = Encoder::new();
        encoder.marker(Marker::Free);
        encoder.marker(Marker::Parameters);
        encoder.count(arity);
        encoder.text(signature);
        encoder.finish()
    }

    pub(crate) fn variant(payload: &[type_ref::Type], owner: type_def::Identity) -> Self {
        let mut encoder = Encoder::new();
        encoder.marker(Marker::Associated);
        encoder.marker(Marker::Parameters);
        encoder.count(payload.len());
        for ty in payload {
            write_type(&mut encoder, ty);
        }
        encoder.marker(Marker::Output);
        encoder.marker(Marker::Script);
        encoder.script(owner);
        encoder.finish()
    }
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
                    encoder.text(path.name.clone());
                }
                type_ref::Target::Parameter | type_ref::Target::Projection(..) | type_ref::Target::Alias(_) => {
                    encoder.marker(Marker::Builtin);
                    encoder.text(ImmutableString::from(format!("<{}>", path.name)));
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
        writer.tag(0x2f00);
        writer.count(self.0.atoms.len());
        for atom in &self.0.atoms {
            match atom {
                Atom::Marker(marker) => writer.tag(signature_tag(*marker)),
                Atom::Count(count) => writer.count(*count),
                Atom::Text(text) => writer.text(text),
                Atom::Script(key) => {
                    writer.tag(0x2f81);
                    key.write(writer)?;
                }
                Atom::Host(path) => {
                    writer.tag(0x2f82);
                    writer.text(path);
                }
                Atom::OpaqueHost => {
                    return Err(crate::preview::digest::Error::OpaqueHostIdentity {
                        context: "preview callback signature identity",
                    });
                }
            }
        }
        Ok(())
    }
}

fn signature_tag(marker: Marker) -> u16 {
    match marker {
        Marker::Version1 => 0x2f01,
        Marker::Free => 0x2f02,
        Marker::Associated => 0x2f03,
        Marker::Owned => 0x2f04,
        Marker::Shared => 0x2f05,
        Marker::Mutable => 0x2f06,
        Marker::Parameters => 0x2f07,
        Marker::Output => 0x2f08,
        Marker::Unit => 0x2f09,
        Marker::Tuple => 0x2f0a,
        Marker::Path => 0x2f0b,
        Marker::Builtin => 0x2f0c,
        Marker::Host => 0x2f0d,
        Marker::Script => 0x2f0e,
        Marker::Arguments => 0x2f0f,
        Marker::TypeArgument => 0x2f10,
        Marker::ErasedLifetime => 0x2f11,
        Marker::SharedReference => 0x2f12,
        Marker::MutableReference => 0x2f13,
    }
}

#[cfg(test)]
mod tests {
    use super::Fingerprint;
    use crate::receiver;
    use crate::resolve::type_ref::Type;
    use crate::Span;

    #[test]
    fn receiver_modes_have_distinct_callable_fingerprints() {
        let owned = Fingerprint::function(true, Some(receiver::Mode::Owned), &[], None);
        let shared = Fingerprint::function(true, Some(receiver::Mode::Shared), &[], None);
        let mutable = Fingerprint::function(true, Some(receiver::Mode::Mutable), &[], None);

        assert_ne!(owned, shared);
        assert_ne!(owned, mutable);
        assert_ne!(shared, mutable);
    }

    #[test]
    fn clones_share_parts_and_equal_fingerprints_compare_by_structure() {
        let owned = Fingerprint::function(true, Some(receiver::Mode::Owned), &[], None);
        let clone = owned.clone();
        let rebuilt = Fingerprint::function(true, Some(receiver::Mode::Owned), &[], None);
        let shared = Fingerprint::function(true, Some(receiver::Mode::Shared), &[], None);

        assert!(crate::Shared::ptr_eq(&owned.0, &clone.0));
        assert!(!crate::Shared::ptr_eq(&owned.0, &rebuilt.0));
        assert_eq!(owned, clone);
        assert_eq!(owned, rebuilt);
        assert_eq!(owned.cmp(&rebuilt), std::cmp::Ordering::Equal);
        assert_ne!(clone, shared);
        assert_eq!(owned.cmp(&shared), rebuilt.cmp(&shared));
    }

    #[test]
    fn structural_tuple_shape_and_order_have_distinct_callable_fingerprints() {
        let unit = Type::Unit(Span::NONE);
        let reference = Type::Reference {
            mutable: false,
            lifetime: None,
            target: Box::new(unit.clone()),
            span: Span::NONE,
        };
        let tuple = Type::Tuple {
            members: [unit.clone(), reference.clone()].into(),
            span: Span::NONE,
        };
        let reversed = Type::Tuple {
            members: [reference, unit.clone()].into(),
            span: Span::NONE,
        };

        assert_ne!(
            Fingerprint::function(false, None, &[], Some(&tuple)),
            Fingerprint::function(false, None, &[], Some(&reversed))
        );
        assert_ne!(
            Fingerprint::function(false, None, &[], Some(&tuple)),
            Fingerprint::function(false, None, &[], Some(&unit))
        );
    }
}
