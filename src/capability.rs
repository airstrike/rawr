//! Compile-time capabilities promised by resolved nominal types.

use crate::resolve::{item, type_ref, Error, LocatedError};
use crate::{program, type_def, ImmutableString, Span};
use std::collections::BTreeMap;

/// A capability that can be derived by a Rawr nominal type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    /// A value can be produced without arguments.
    Default,
    /// A value has a deterministic debug representation.
    Debug,
    /// A value can be cloned under Rawr value semantics.
    Clone,
    /// A value can be copied under Rawr value semantics.
    Copy,
    /// A value supports partial equality.
    PartialEq,
    /// A value supports total equality.
    Eq,
    /// A value supports partial ordering (`#[derive(PartialOrd)]`).
    PartialOrd,
    /// A value supports total ordering (`#[derive(Ord)]`).
    Ord,
    /// A value can key a hashed collection (`#[derive(Hash)]`).
    Hash,
    /// A value may cross the framework's thread boundary.
    Send,
    /// A value contains no non-static borrowed data.
    Static,
}

impl Kind {
    const ALL: [Self; 11] = [
        Self::Default,
        Self::Debug,
        Self::Clone,
        Self::Copy,
        Self::PartialEq,
        Self::Eq,
        Self::PartialOrd,
        Self::Ord,
        Self::Hash,
        Self::Send,
        Self::Static,
    ];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "Default" => Some(Self::Default),
            "Debug" => Some(Self::Debug),
            "Clone" => Some(Self::Clone),
            "Copy" => Some(Self::Copy),
            "PartialEq" => Some(Self::PartialEq),
            "Eq" => Some(Self::Eq),
            "Hash" => Some(Self::Hash),
            _ => None,
        }
    }

    /// A derivable std trait: the capabilities `from_name` reads in bounds,
    /// and `PartialOrd` and `Ord`, whose bounds are traits.
    pub(crate) fn from_derive(name: &str) -> Option<Self> {
        match name {
            "PartialOrd" => Some(Self::PartialOrd),
            "Ord" => Some(Self::Ord),
            name => Self::from_name(name),
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Debug => "Debug",
            Self::Clone => "Clone",
            Self::Copy => "Copy",
            Self::PartialEq => "PartialEq",
            Self::Eq => "Eq",
            Self::PartialOrd => "PartialOrd",
            Self::Ord => "Ord",
            Self::Hash => "Hash",
            Self::Send => "Send",
            Self::Static => "Static",
        }
    }

    const fn bit(self) -> u16 {
        match self {
            Self::Default => 1 << 0,
            Self::Debug => 1 << 1,
            Self::Clone => 1 << 2,
            Self::Copy => 1 << 3,
            Self::PartialEq => 1 << 4,
            Self::Eq => 1 << 5,
            Self::Send => 1 << 6,
            Self::Static => 1 << 7,
            Self::PartialOrd => 1 << 8,
            Self::Ord => 1 << 9,
            Self::Hash => 1 << 10,
        }
    }
}

/// An opaque set proving the capabilities validated for one type definition.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Set(u16);

/// An exact canonical set of capabilities attached to a symbolic item.
///
/// The representation is compared directly and is not a serialized digest or
/// a cross-version compatibility promise.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint(Box<[Name]>);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Name {
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Send,
    Static,
}

impl Set {
    /// Create an empty capability set.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }

    /// Create a canonical set from explicit capability declarations.
    #[must_use]
    pub fn from_explicit(capabilities: impl IntoIterator<Item = Kind>) -> Self {
        let mut set = Self::new();
        for capability in capabilities {
            set.insert(capability);
        }
        set
    }

    /// Return whether this set contains a capability.
    #[must_use]
    pub const fn contains(self, capability: Kind) -> bool {
        self.0 & capability.bit() != 0
    }

    fn insert(&mut self, capability: Kind) {
        self.0 |= capability.bit();
    }

    fn remove(&mut self, capability: Kind) {
        self.0 &= !capability.bit();
    }

    pub(crate) fn fingerprint(self) -> Fingerprint {
        let capabilities = Kind::ALL
            .into_iter()
            .filter(|capability| self.contains(*capability))
            .map(Name::from)
            .collect();
        Fingerprint(capabilities)
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = Kind> {
        Kind::ALL
            .into_iter()
            .filter(move |capability| self.contains(*capability))
    }
}

impl Default for Fingerprint {
    fn default() -> Self {
        Set::default().fingerprint()
    }
}

impl From<Kind> for Name {
    fn from(value: Kind) -> Self {
        match value {
            Kind::Default => Self::Default,
            Kind::Debug => Self::Debug,
            Kind::Clone => Self::Clone,
            Kind::Copy => Self::Copy,
            Kind::PartialEq => Self::PartialEq,
            Kind::Eq => Self::Eq,
            Kind::PartialOrd => Self::PartialOrd,
            Kind::Ord => Self::Ord,
            Kind::Hash => Self::Hash,
            Kind::Send => Self::Send,
            Kind::Static => Self::Static,
        }
    }
}

impl crate::preview::digest::Write for Set {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2200);
        writer.count(self.iter().count());
        for capability in self.iter() {
            write_kind(writer, capability);
        }
        Ok(())
    }
}

impl crate::preview::digest::Write for Fingerprint {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        writer.tag(0x2210);
        writer.count(self.0.len());
        for capability in &self.0 {
            writer.tag(match capability {
                Name::Default => 0x2221,
                Name::Debug => 0x2222,
                Name::Clone => 0x2223,
                Name::Copy => 0x2224,
                Name::PartialEq => 0x2225,
                Name::Eq => 0x2226,
                Name::Send => 0x2227,
                Name::Static => 0x2228,
                Name::PartialOrd => 0x2229,
                Name::Ord => 0x222a,
                Name::Hash => 0x222b,
            });
        }
        Ok(())
    }
}

fn write_kind(writer: &mut crate::preview::digest::Writer, capability: Kind) {
    writer.tag(match capability {
        Kind::Default => 0x2231,
        Kind::Debug => 0x2232,
        Kind::Clone => 0x2233,
        Kind::Copy => 0x2234,
        Kind::PartialEq => 0x2235,
        Kind::Eq => 0x2236,
        Kind::Send => 0x2237,
        Kind::Static => 0x2238,
        Kind::PartialOrd => 0x2239,
        Kind::Ord => 0x223a,
        Kind::Hash => 0x223b,
    });
}

#[derive(Clone)]
struct Request {
    source: program::source::Id,
    owner: ImmutableString,
    owner_span: Span,
    kind: type_def::Kind,
    members: Box<[type_ref::Type]>,
    requested: Set,
    spans: BTreeMap<Kind, Span>,
    /// An enum's derived `Default`: `Ok` with one `#[default]` unit
    /// variant, else Rust's reason.
    enum_default: Result<(), &'static str>,
}

pub(crate) fn resolve(items: &[item::Shell]) -> Result<BTreeMap<type_def::Id, Set>, LocatedError> {
    let mut requests = BTreeMap::new();

    for shell in items {
        let request = match &shell.kind {
            item::Kind::Struct(item) => Request::new(
                shell.source,
                &item.header,
                &item.name.text,
                item.name.span,
                type_def::Kind::Struct,
                item.fields
                    .iter()
                    .map(|field| {
                        // A `&str` field is a shared reference: `Copy`.
                        if field.shared_str {
                            type_ref::Type::Reference {
                                mutable: false,
                                lifetime: None,
                                target: Box::new(field.ty.clone()),
                                span: field.span,
                            }
                        } else {
                            field.ty.clone()
                        }
                    })
                    .collect(),
            )
            .map_err(|error| LocatedError::primary(shell.source, error))?,
            item::Kind::Enum(item) => Request::new(
                shell.source,
                &item.header,
                &item.name.text,
                item.name.span,
                type_def::Kind::Enum,
                item.variants
                    .iter()
                    .flat_map(|variant| variant.payload.iter().cloned())
                    .collect(),
            )
            .map(|request| Request {
                enum_default: match item
                    .variants
                    .iter()
                    .filter(|variant| variant.default.is_some())
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    [variant] if variant.payload.is_empty() => Ok(()),
                    [_] => Err("the `#[default]` attribute may only be used on unit enum variants"),
                    [] => Err("no default declared: mark a unit variant `#[default]`"),
                    _ => Err("multiple declared defaults"),
                },
                ..request
            })
            .map_err(|error| LocatedError::primary(shell.source, error))?,
            item::Kind::Use(_)
            | item::Kind::Impl(_)
            | item::Kind::Trait(_)
            | item::Kind::Function(_) => continue,
        };

        let definition = match &shell.kind {
            item::Kind::Struct(item) => item.definition,
            item::Kind::Enum(item) => item.definition,
            item::Kind::Use(_)
            | item::Kind::Impl(_)
            | item::Kind::Trait(_)
            | item::Kind::Function(_) => {
                unreachable!("only nominal items create capability requests")
            }
        };
        requests.insert(definition, request);
    }

    // Structural capabilities are coinductive: a recursive enum can provide
    // `Clone`, for example, when every way out of the cycle also provides it.
    // `Send` and `Static` are facts proven for every script nominal rather
    // than source-requested derives, so they enter the same greatest fixed
    // point even when they do not appear in an attribute. `Default` is
    // constructive instead, so it starts empty and is added only after all
    // fields already have a finite default proof.
    // A hand-written `impl Clone for T` (or `PartialEq`, `Eq`, `Copy`,
    // `Default`, `Debug`) provides the capability outright, as an impl does
    // in Rust; deriving it as well is a conflicting implementation.
    let mut manual = BTreeMap::<type_def::Id, Set>::new();
    for shell in items {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        let Some(item::TraitImpl::User { name, .. }) = &implementation.trait_ else {
            continue;
        };
        let Some(capability) = Kind::from_derive(&name.text) else {
            continue;
        };
        let type_ref::Type::Path(type_ref::Path {
            target: type_ref::Target::Script { definition, .. },
            ..
        }) = &implementation.owner
        else {
            continue;
        };
        if let Some(request) = requests.get(definition)
            && request.requested.contains(capability)
        {
            return Err(LocatedError::primary(
                shell.source,
                Error::InvalidDerive {
                    owner: request.owner.clone(),
                    capability: capability.name().into(),
                    reason: "conflicting implementations: it is both derived and implemented"
                        .into(),
                    attribute: name.span,
                    declaration: request.owner_span,
                },
            ));
        }
        manual.entry(*definition).or_default().insert(capability);
    }

    let mut resolved: BTreeMap<_, _> = requests
        .iter()
        .map(|(&definition, request)| {
            let mut candidates = request.requested;
            candidates.remove(Kind::Default);
            candidates.insert(Kind::Send);
            candidates.insert(Kind::Static);
            for capability in manual
                .get(&definition)
                .into_iter()
                .flat_map(|set| set.iter())
            {
                candidates.insert(capability);
            }
            (definition, candidates)
        })
        .collect();

    loop {
        let previous = resolved.clone();
        let mut next = previous.clone();

        for (&definition, request) in &requests {
            let set = next
                .get_mut(&definition)
                .expect("each capability request has one result set");

            for capability in Kind::ALL {
                if manual
                    .get(&definition)
                    .is_some_and(|set| set.contains(capability))
                {
                    continue;
                }
                if !request.requested.contains(capability)
                    && !matches!(capability, Kind::Send | Kind::Static)
                {
                    continue;
                }
                if capability == Kind::Default {
                    if !set.contains(capability)
                        && request.supports(definition, capability, &previous)
                    {
                        set.insert(capability);
                    }
                } else if set.contains(capability)
                    && !request.supports(definition, capability, &previous)
                {
                    set.remove(capability);
                }
            }
        }

        if next == previous {
            break;
        }
        resolved = next;
    }

    for (&definition, request) in &requests {
        let set = resolved
            .get(&definition)
            .expect("each capability request has one result set");

        for capability in Kind::ALL {
            if request.requested.contains(capability) && !set.contains(capability) {
                return Err(LocatedError::primary(
                    request.source,
                    Error::InvalidDerive {
                        owner: request.owner.clone(),
                        capability: capability.name().into(),
                        reason: request.reason(definition, capability, &resolved).into(),
                        attribute: request.spans[&capability],
                        declaration: request.owner_span,
                    },
                ));
            }
        }
    }

    Ok(resolved)
}

impl Request {
    fn new(
        source: program::source::Id,
        header: &crate::item::Header,
        owner: &str,
        owner_span: Span,
        kind: type_def::Kind,
        members: Box<[type_ref::Type]>,
    ) -> Result<Self, Error> {
        let mut requested = Set::default();
        let mut spans = BTreeMap::new();

        for attribute in &header.attributes {
            if attribute.path.segments.len() != 1
                || attribute.path.segments[0].name.text != "derive"
            {
                continue;
            }

            for argument in &attribute.arguments {
                let name = argument
                    .segments
                    .iter()
                    .map(|segment| segment.name.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                let Some(capability) = Kind::from_derive(&name) else {
                    return Err(Error::InvalidDerive {
                        owner: owner.into(),
                        capability: name.into(),
                        reason: "unsupported derive".into(),
                        attribute: argument.span,
                        declaration: owner_span,
                    });
                };
                if spans.insert(capability, argument.span).is_some() {
                    return Err(Error::InvalidDerive {
                        owner: owner.into(),
                        capability: capability.name().into(),
                        reason: "capability is derived more than once".into(),
                        attribute: argument.span,
                        declaration: owner_span,
                    });
                }
                requested.insert(capability);
            }
        }

        Ok(Self {
            source,
            owner: owner.into(),
            owner_span,
            kind,
            members,
            requested,
            spans,
            enum_default: Ok(()),
        })
    }

    fn supports(
        &self,
        definition: type_def::Id,
        capability: Kind,
        resolved: &BTreeMap<type_def::Id, Set>,
    ) -> bool {
        if capability == Kind::Default && self.kind == type_def::Kind::Enum {
            return self.enum_default.is_ok();
        }
        if capability == Kind::Copy && !resolved[&definition].contains(Kind::Clone) {
            return false;
        }
        if capability == Kind::Eq && !resolved[&definition].contains(Kind::PartialEq) {
            return false;
        }
        // `Ord: Eq + PartialOrd`, `PartialOrd: PartialEq`.
        if capability == Kind::PartialOrd && !resolved[&definition].contains(Kind::PartialEq) {
            return false;
        }
        if capability == Kind::Ord
            && !(resolved[&definition].contains(Kind::Eq)
                && resolved[&definition].contains(Kind::PartialOrd))
        {
            return false;
        }

        self.members
            .iter()
            .all(|ty| type_supports(ty, capability, resolved))
    }

    fn reason(
        &self,
        definition: type_def::Id,
        capability: Kind,
        resolved: &BTreeMap<type_def::Id, Set>,
    ) -> &'static str {
        if capability == Kind::Default
            && let Err(reason) = self.enum_default
        {
            return reason;
        }
        if capability == Kind::Copy && !self.requested.contains(Kind::Clone) {
            return "Copy requires Clone";
        }
        if capability == Kind::Copy && !resolved[&definition].contains(Kind::Clone) {
            return "Clone could not be proven";
        }
        if capability == Kind::Eq && !self.requested.contains(Kind::PartialEq) {
            return "Eq requires PartialEq";
        }
        if capability == Kind::Eq && !resolved[&definition].contains(Kind::PartialEq) {
            return "PartialEq could not be proven";
        }
        if capability == Kind::PartialOrd && !resolved[&definition].contains(Kind::PartialEq) {
            return "PartialOrd requires PartialEq";
        }
        if capability == Kind::Ord
            && !(resolved[&definition].contains(Kind::Eq)
                && resolved[&definition].contains(Kind::PartialOrd))
        {
            return "Ord requires Eq and PartialOrd";
        }
        if self
            .members
            .iter()
            .any(|ty| !type_supports(ty, capability, resolved))
        {
            return "a field or variant payload does not provide this capability";
        }
        "capability requirements are not satisfied"
    }
}

pub(crate) fn type_supports(
    ty: &type_ref::Type,
    capability: Kind,
    resolved: &BTreeMap<type_def::Id, Set>,
) -> bool {
    match ty {
        type_ref::Type::Unit(_) => true,
        type_ref::Type::Tuple { members, .. } => members
            .iter()
            .all(|member| type_supports(member, capability, resolved)),
        // A shared `&T` is `Copy` and `Clone`, and derives the comparison
        // and `Debug` traits from `T`.
        type_ref::Type::Reference {
            mutable: false,
            target,
            ..
        } => match capability {
            Kind::Clone | Kind::Copy => true,
            Kind::Debug
            | Kind::PartialEq
            | Kind::Eq
            | Kind::PartialOrd
            | Kind::Ord
            | Kind::Hash => type_supports(target, capability, resolved),
            _ => false,
        },
        type_ref::Type::Reference { .. } => false,
        // `Option<T>` is `Default`, and otherwise derives from `T`.
        type_ref::Type::Path(_) if let Some(element) = ty.option_element() => {
            capability == Kind::Default || type_supports(element, capability, resolved)
        }
        // `Vec<T>` is `Default`, and otherwise derives from `T`.
        type_ref::Type::Path(_) if let Some(element) = ty.vec_element() => {
            capability == Kind::Default || type_supports(element, capability, resolved)
        }
        // `[T; N]` derives from `T`; Rust's `Default` stops at 32 elements
        // (and holds for `[T; 0]` whatever `T`).
        type_ref::Type::Path(_) if let Some((element, length)) = ty.array_element() => {
            match capability {
                Kind::Default if length == 0 => true,
                Kind::Default if length > 32 => false,
                _ => type_supports(element, capability, resolved),
            }
        }
        // Maps and sets are `Default`, never `Copy`, and otherwise derive
        // from their arguments; only the B-tree kinds are ordered or `Hash`.
        type_ref::Type::Path(_) if let Some((name, arguments)) = ty.keyed_arguments() => {
            match capability {
                Kind::Default => true,
                Kind::Copy => false,
                Kind::PartialOrd | Kind::Ord | Kind::Hash if name.starts_with("Hash") => false,
                _ => arguments
                    .iter()
                    .all(|argument| type_supports(argument, capability, resolved)),
            }
        }
        // `RefCell<T>` and `Cell<T>` are never `Copy` or `Hash`, and
        // otherwise derive from `T`. A `Cell`'s value is copied out, so its
        // traits other than `Default`, `Send` and `Static` also need
        // `T: Copy`.
        type_ref::Type::Path(_) if let Some((cell, element)) = ty.cell_element() => {
            !matches!(capability, Kind::Copy | Kind::Hash)
                && (cell == "RefCell"
                    || matches!(capability, Kind::Default | Kind::Send | Kind::Static)
                    || type_supports(element, Kind::Copy, resolved))
                && type_supports(element, capability, resolved)
        }
        // std's `dyn Error` is `Debug`, its supertrait.
        type_ref::Type::Path(_) if ty.dyn_trait() == Some("Error") => capability == Kind::Debug,
        // `Reverse<T>` derives everything from `T`.
        type_ref::Type::Path(_) if let Some(element) = ty.reverse_element() => {
            type_supports(element, capability, resolved)
        }
        // `Box<T>` derives from `T`, and is never `Copy`. `Rc<T>` and
        // `Arc<T>` are `Clone` for any `T`, and an `Rc` is not `Send`.
        type_ref::Type::Path(_) if let Some((pointer, element)) = ty.pointer_element() => {
            match (pointer, capability) {
                (_, Kind::Copy) | ("Rc", Kind::Send) => false,
                ("Rc" | "Arc", Kind::Clone) => true,
                _ => type_supports(element, capability, resolved),
            }
        }
        // A function type is `Copy`, `Clone`, `Debug` and the marker `Eq`;
        // Rawr does not compare, hash or default function values.
        type_ref::Type::Path(_) if ty.fn_signature().is_some() => {
            matches!(
                capability,
                Kind::Clone | Kind::Copy | Kind::Debug | Kind::Eq
            )
        }
        // `Result<T, E>` is never `Default`, and otherwise derives from both.
        type_ref::Type::Path(_) if let Some((ok, err)) = ty.result_elements() => {
            capability != Kind::Default
                && type_supports(ok, capability, resolved)
                && type_supports(err, capability, resolved)
        }
        type_ref::Type::Path(path) => match &path.target {
            type_ref::Target::Builtin => builtin_supports(&path.name, capability),
            // A generic parameter's capabilities are checked where it is
            // instantiated, as a derive's bounds are.
            type_ref::Target::Parameter
            | type_ref::Target::Projection(..)
            | type_ref::Target::Alias(_) => true,
            type_ref::Target::Host { definition, .. } => {
                definition.capabilities().contains(capability)
            }
            type_ref::Target::Script { definition, .. } => resolved
                .get(definition)
                .is_some_and(|set| set.contains(capability)),
        },
    }
}

pub(crate) fn builtin_supports(name: &str, capability: Kind) -> bool {
    match crate::host::runtime_builtin(name) {
        "String" => capability != Kind::Copy,
        "f64" => !matches!(capability, Kind::Eq | Kind::Ord | Kind::Hash),
        "bool" | "char" | "i64" | "i128" => true,
        // `std::cmp::Ordering` derives all but `Default`.
        "Ordering" => capability != Kind::Default,
        // `std::time::Duration` has them all; `Instant` all but `Default`.
        "Duration" => true,
        "Instant" => capability != Kind::Default,
        // `std::ops::RangeInclusive` is neither `Copy` nor ordered.
        "RangeInclusive" => !matches!(capability, Kind::Copy | Kind::PartialOrd | Kind::Ord),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Kind;
    use crate::host::facade;
    use crate::{capability, resolve, Engine};

    struct Opaque;

    #[test]
    fn send_and_static_are_intrinsic_coinductive_facts() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit(
                "struct Leaf { text: String } \
                 enum Cycle { End, Next(Cycle) } \
                 struct Root { leaf: Leaf, cycle: Cycle }",
            )
            .unwrap();
        let program = resolve::resolve(unit, &engine.host_registry).unwrap();

        for definition in program.types() {
            let capabilities = definition.capabilities();
            assert!(capabilities.contains(Kind::Send));
            assert!(capabilities.contains(Kind::Static));
            assert!(!capabilities.contains(Kind::Clone));
        }
    }

    #[test]
    fn references_break_intrinsic_structural_facts() {
        let engine = Engine::new();
        let unit = engine
            .parse_unit("struct Borrowed { value: &i64 }")
            .unwrap();
        let program = resolve::resolve(unit, &engine.host_registry).unwrap();
        let capabilities = program.types()[0].capabilities();

        assert!(!capabilities.contains(Kind::Send));
        assert!(!capabilities.contains(Kind::Static));
    }

    #[test]
    fn host_edges_require_each_explicit_structural_fact() {
        let mut engine = Engine::new();
        let mut builder = facade::Builder::new("host", facade::Abi::new(1).unwrap()).unwrap();
        builder
            .type_::<Opaque>("Opaque", &[], &[capability::Kind::Send])
            .unwrap();
        engine
            .register_host_facade(builder.finish().unwrap())
            .unwrap();

        let unit = engine
            .parse_unit("use host::Opaque; struct Wrapper { value: Opaque }")
            .unwrap();
        let program = resolve::resolve(unit, &engine.host_registry).unwrap();
        let capabilities = program.types()[0].capabilities();

        assert!(capabilities.contains(Kind::Send));
        assert!(!capabilities.contains(Kind::Static));
    }

    #[test]
    fn structural_facts_are_not_parseable_derives() {
        for capability in ["Send", "Static"] {
            let source = format!("#[derive({capability})] struct Value;");
            let error = Engine::new().compile(&source).unwrap_err();
            assert!(error.to_string().contains("unsupported derive"), "{error}");
        }
    }
}
