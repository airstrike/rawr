//! What a call's value keeps of what the call takes, read from the callee's
//! signature as rustc reads it (D572.4): lifetime elision, the lifetimes
//! written in it and the outlives bounds on them, `'static`, and its type
//! parameters.
//!
//! A value's borrows are one set in these facts, so an input whose regions
//! the output keeps only some of can't be followed: that input is
//! [`Keeps::Unknown`], and the call is left to rustc (D572.5).

use crate::resolve::item::Lifetimes;
use crate::resolve::type_ref::{Argument, Target, Type};
use crate::{ImmutableString, Span};

/// What a call's value, or a place the call stores into, keeps of one of the
/// call's inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Keeps {
    /// None of its borrows.
    Nothing,
    /// All of them: a reference's own loan and what it refers to.
    All,
    /// What a reference refers to, without its own loan: `T` of `&'a T`
    /// for an output `T`. For an input that is not a reference, all of it.
    Inner,
    /// Some of its regions and not others, which these facts can't tell
    /// apart, or regions the signature does not say.
    Unknown,
}

/// How a method takes its receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Receiver {
    Shared,
    Mutable,
    Owned,
}

/// A callee's signature, as the borrow checker reads it.
pub(crate) struct Signature<'a> {
    pub(crate) receiver: Option<Receiver>,
    pub(crate) parameters: Vec<&'a Type>,
    pub(crate) output: &'a Type,
    pub(crate) lifetimes: &'a Lifetimes,
}

/// What a call keeps of each of its inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    /// How the method takes its receiver.
    pub(crate) mode: Option<Receiver>,
    /// Of the receiver, in the call's value.
    pub(crate) receiver: Keeps,
    /// Of each argument, in the call's value.
    pub(crate) parameters: Vec<Keeps>,
    /// Of each argument, in what a `&mut self` receiver holds afterwards
    /// (`v.push(&x)`).
    pub(crate) stores: Vec<Keeps>,
    /// Of each argument, borrowed for `'static`.
    pub(crate) statics: Vec<Keeps>,
    /// A `&mut` parameter other than the receiver may take another input's
    /// borrows, which these facts do not follow yet.
    pub(crate) unknown_stores: bool,
}

impl Summary {
    /// A call whose value keeps nothing it takes.
    pub(crate) fn nothing(mode: Option<Receiver>, arguments: usize) -> Self {
        Self {
            mode,
            receiver: Keeps::Nothing,
            parameters: vec![Keeps::Nothing; arguments],
            stores: vec![Keeps::Nothing; arguments],
            statics: vec![Keeps::Nothing; arguments],
            unknown_stores: false,
        }
    }

    /// A call whose value may keep any of what it takes.
    pub(crate) fn unknown(arguments: usize) -> Self {
        Self {
            mode: None,
            receiver: Keeps::Unknown,
            parameters: vec![Keeps::Unknown; arguments],
            stores: vec![Keeps::Nothing; arguments],
            statics: vec![Keeps::Nothing; arguments],
            unknown_stores: true,
        }
    }

    /// A builtin method taking its receiver as `mode`, whose value keeps
    /// `receiver` of it and `parameters` of its arguments, and which stores
    /// `stores` of them in its receiver. Arguments past those listed keep
    /// nothing.
    pub(crate) fn builtin(
        mode: Receiver,
        receiver: Keeps,
        parameters: &[Keeps],
        stores: &[Keeps],
        arguments: usize,
    ) -> Self {
        let padded = |listed: &[Keeps]| {
            (0..arguments)
                .map(|index| listed.get(index).copied().unwrap_or(Keeps::Nothing))
                .collect()
        };
        Self {
            mode: Some(mode),
            receiver,
            parameters: padded(parameters),
            stores: padded(stores),
            statics: vec![Keeps::Nothing; arguments],
            unknown_stores: false,
        }
    }
}

/// A region of a type in a signature.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Region {
    Named(ImmutableString),
    /// An elided lifetime, numbered in the signature.
    Elided(usize),
    Static,
    /// A type parameter: whatever regions its argument has.
    Parameter(ImmutableString),
    /// An `impl Trait` output, which captures every input's regions
    /// (edition 2024).
    Captures,
    /// A region the signature does not say (a trait object's, a function
    /// type's, a facade type's).
    Unknown,
}

/// A type's regions: its own reference's, and those below it.
#[derive(Default)]
struct Shape {
    outer: Option<Region>,
    inner: Vec<Region>,
}

impl Shape {
    fn all(&self) -> impl Iterator<Item = &Region> {
        self.outer.iter().chain(&self.inner)
    }
}

struct Reader<'a> {
    lifetimes: &'a Lifetimes,
    /// How many lifetime parameters a crate type declares.
    declared: &'a dyn Fn(&Target) -> Option<usize>,
    /// Where each elided lifetime is, by its number less one.
    elided: Vec<Span>,
}

impl Reader<'_> {
    /// The lifetime written at `span` in a type at `at`, or an elided one.
    fn lifetime(&mut self, span: Option<Span>, at: Span) -> Region {
        match span.and_then(|span| self.lifetimes.names.get(&span)) {
            Some(name) if name == "static" => Region::Static,
            Some(name) if name != "_" => Region::Named(name.clone()),
            _ => self.fresh(at),
        }
    }

    /// An elided lifetime of the type at `at`.
    fn fresh(&mut self, at: Span) -> Region {
        self.elided.push(at);
        Region::Elided(self.elided.len())
    }

    fn shape(&mut self, ty: &Type) -> Shape {
        match ty {
            Type::Unit(_) => Shape::default(),
            Type::Tuple { members, .. } => Shape {
                outer: None,
                inner: members
                    .iter()
                    .flat_map(|member| self.regions(member))
                    .collect(),
            },
            Type::Reference {
                lifetime,
                target,
                span,
                ..
            } => Shape {
                outer: Some(self.lifetime(*lifetime, *span)),
                inner: self.regions(target),
            },
            // `&str`, which a resolved type reads as `String` with the
            // reference's lifetime.
            Type::Path(path) if ty.is_str() => {
                let span = match &*path.arguments {
                    [Argument::Lifetime(span)] => Some(*span),
                    _ => None,
                };
                Shape {
                    outer: Some(self.lifetime(span, path.span)),
                    inner: Vec::new(),
                }
            }
            Type::Path(_) if ty.iterator_item().is_some() => Shape {
                outer: None,
                inner: vec![Region::Captures],
            },
            // A function value holds what it captures: within the
            // lifetimes its type is bounded by (`impl Fn() + 'a`), or any.
            Type::Path(path) if ty.fn_signature().is_some() => {
                let bounded: Vec<Region> = path
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Argument::Lifetime(span) => Some(self.lifetime(Some(*span), *span)),
                        Argument::Type(_) => None,
                    })
                    .collect();
                Shape {
                    outer: None,
                    inner: if bounded.is_empty() {
                        vec![Region::Unknown]
                    } else {
                        bounded
                    },
                }
            }
            Type::Path(_) if ty.dyn_trait().is_some() => Shape {
                outer: None,
                inner: vec![Region::Unknown],
            },
            Type::Path(path) => {
                let mut inner = Vec::new();
                match &path.target {
                    Target::Parameter if path.name == "Self" => match &self.lifetimes.owner {
                        Some(owner) => {
                            let owner = owner.clone();
                            inner.extend(self.regions(&owner));
                        }
                        None => inner.push(Region::Unknown),
                    },
                    Target::Parameter => inner.push(Region::Parameter(path.name.clone())),
                    Target::Projection(..) | Target::Alias(_) | Target::Host { .. } => {
                        inner.push(Region::Unknown);
                    }
                    Target::Builtin if opaque(&path.name) => {
                        inner.push(Region::Captures);
                    }
                    Target::Builtin | Target::Script { .. } => {}
                }
                let mut written = 0;
                for argument in &path.arguments {
                    match argument {
                        Argument::Type(ty) => inner.extend(self.regions(ty)),
                        Argument::Lifetime(span) => {
                            written += 1;
                            inner.push(self.lifetime(Some(*span), *span));
                        }
                    }
                }
                // `Wrapper` of `struct Wrapper<'a>`: its lifetimes elided.
                if matches!(path.target, Target::Script { .. }) {
                    match (self.declared)(&path.target) {
                        Some(declared) => {
                            for _ in written..declared {
                                let fresh = self.fresh(path.span);
                                inner.push(fresh);
                            }
                        }
                        None => inner.push(Region::Unknown),
                    }
                }
                Shape { outer: None, inner }
            }
        }
    }

    fn regions(&mut self, ty: &Type) -> Vec<Region> {
        let shape = self.shape(ty);
        shape.outer.into_iter().chain(shape.inner).collect()
    }
}

/// Whether a builtin path named `name` is an imported trait's `impl Trait`
/// output (`type_ref::OPAQUE_PREFIX`).
fn opaque(name: &str) -> bool {
    #[cfg(feature = "rust-import")]
    return name.starts_with(crate::resolve::type_ref::OPAQUE_PREFIX);
    #[cfg(not(feature = "rust-import"))]
    {
        let _ = name;
        false
    }
}

/// Whether `longer: shorter` by the signature's outlives bounds.
fn outlives(bounds: &[(ImmutableString, ImmutableString)], longer: &str, shorter: &str) -> bool {
    let mut seen = vec![longer];
    let mut index = 0;
    while let Some(&at) = seen.get(index) {
        if at == shorter {
            return true;
        }
        for (from, to) in bounds {
            if from == at && !seen.contains(&to.as_str()) {
                seen.push(to);
            }
        }
        index += 1;
    }
    false
}

/// Whether a region of an input must outlive one of `into`: whether what
/// it borrows is kept there. `None` when the signature does not say.
fn relates(
    region: &Region,
    into: &[Region],
    bounds: &[(ImmutableString, ImmutableString)],
) -> Option<bool> {
    let into: Vec<&Region> = into.iter().filter(|r| **r != Region::Static).collect();
    if into.is_empty() || *region == Region::Static {
        return Some(false);
    }
    if into.contains(&&Region::Captures) {
        return Some(true);
    }
    if into.contains(&&Region::Unknown) || *region == Region::Unknown {
        return None;
    }
    Some(into.iter().any(|kept| {
        match (region, kept) {
            (Region::Named(longer), Region::Named(shorter)) => outlives(bounds, longer, shorter),
            (Region::Elided(a), Region::Elided(b)) => a == b,
            (Region::Parameter(a), Region::Parameter(b)) => a == b,
            // `T: 'a` keeps `T`'s borrows in `'a`.
            (Region::Parameter(parameter), Region::Named(shorter)) => bounds
                .iter()
                .any(|(from, to)| from == parameter && outlives(bounds, to, shorter)),
            _ => false,
        }
    }))
}

/// What of an input of shape `shape` the regions `into` keep.
fn keeps(shape: &Shape, into: &[Region], bounds: &[(ImmutableString, ImmutableString)]) -> Keeps {
    let outer = shape
        .outer
        .as_ref()
        .map(|outer| relates(outer, into, bounds));
    let inner: Vec<Option<bool>> = shape
        .inner
        .iter()
        .map(|region| relates(region, into, bounds))
        .collect();
    if outer == Some(None) || inner.contains(&None) {
        return Keeps::Unknown;
    }
    // A reference kept keeps what it refers to (`&'a T<'b>` has `'b: 'a`).
    if outer == Some(Some(true)) {
        return Keeps::All;
    }
    if inner.iter().all(|kept| *kept == Some(false)) {
        return Keeps::Nothing;
    }
    if inner.iter().all(|kept| *kept == Some(true)) {
        return if shape.outer.is_some() {
            Keeps::Inner
        } else {
            Keeps::All
        };
    }
    Keeps::Unknown
}

/// A signature's regions, as elision gives them.
struct Read {
    /// The impl's owner's, which `self` is.
    owner: Vec<Region>,
    receiver: Option<Shape>,
    parameters: Vec<Shape>,
    /// The output's, elided ones not yet given an input's.
    output: Vec<Region>,
    /// What elision gives the output's elided lifetimes: `&self`'s, or the
    /// one input lifetime. `None` when it gives none, or when the inputs
    /// have regions the signature does not say.
    elided_to: Option<Region>,
    /// The inputs have regions the signature does not say.
    unknown: bool,
    /// Where each elided lifetime is, by its number less one.
    elided: Vec<Span>,
}

fn read(signature: &Signature<'_>, declared: &dyn Fn(&Target) -> Option<usize>) -> Read {
    let mut reader = Reader {
        lifetimes: signature.lifetimes,
        declared,
        elided: Vec::new(),
    };
    let owner = match &signature.lifetimes.owner {
        Some(owner) => reader.regions(owner),
        None if signature.receiver.is_some() => vec![Region::Unknown],
        None => Vec::new(),
    };
    let receiver = signature.receiver.map(|mode| match mode {
        Receiver::Owned => Shape {
            outer: None,
            inner: owner.clone(),
        },
        Receiver::Shared | Receiver::Mutable => {
            let outer = match &signature.lifetimes.receiver {
                Some(name) if name == "static" => Region::Static,
                Some(name) if name != "_" => Region::Named(name.clone()),
                _ => reader.fresh(Span::NONE),
            };
            Shape {
                outer: Some(outer),
                inner: owner.clone(),
            }
        }
    });
    let parameters: Vec<Shape> = signature
        .parameters
        .iter()
        .map(|ty| reader.shape(ty))
        .collect();
    let output = reader.regions(signature.output);

    let mut distinct: Vec<&Region> = Vec::new();
    let mut unknown = false;
    for region in receiver.iter().chain(&parameters).flat_map(Shape::all) {
        match region {
            Region::Named(_) | Region::Elided(_) | Region::Static => {
                if !distinct.contains(&region) {
                    distinct.push(region);
                }
            }
            Region::Unknown => unknown = true,
            Region::Parameter(_) | Region::Captures => {}
        }
    }
    let elided_to = match &receiver {
        Some(Shape {
            outer: Some(outer), ..
        }) => Some(outer.clone()),
        _ => match distinct[..] {
            [only] if !unknown => Some(only.clone()),
            _ => None,
        },
    };
    Read {
        owner,
        receiver,
        parameters,
        output,
        elided_to,
        unknown,
        elided: reader.elided,
    }
}

/// Where the output of `signature` has a lifetime elided that elision
/// gives no input lifetime to: rustc's E0106. `None` when there is none, or
/// when the inputs have regions the signature does not say.
pub(crate) fn missing_lifetime(
    signature: &Signature<'_>,
    declared: &dyn Fn(&Target) -> Option<usize>,
) -> Option<Span> {
    let read = read(signature, declared);
    if read.elided_to.is_some() || read.unknown {
        return None;
    }
    read.output.iter().find_map(|region| match region {
        Region::Elided(number) => read.elided.get(number - 1).copied(),
        _ => None,
    })
}

/// A lifetime a function's own signature names or elides, as its body is
/// checked against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lifetime {
    Named(ImmutableString),
    /// An elided input lifetime, numbered in the signature.
    Elided(usize),
    Static,
}

/// The lifetimes of the borrows an input or the output of a function's own
/// signature holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Borrows {
    /// It holds no borrow.
    None,
    /// All its borrows have this one lifetime.
    One(Lifetime),
    /// A reference of this lifetime to what holds borrows of others, which
    /// outlive it: each borrow it holds outlives this one.
    AtLeast(Lifetime),
    /// Several lifetimes, or ones the signature does not say, which these
    /// facts do not tell apart.
    Several,
}

impl Borrows {
    /// Whether each borrow it holds outlives every one `output` holds, by
    /// the signature's `bounds`: `None` when these facts can't tell.
    pub(crate) fn outlive(
        &self,
        output: &Borrows,
        bounds: &[(ImmutableString, ImmutableString)],
    ) -> Option<bool> {
        match (self, output) {
            (Borrows::None, _) | (_, Borrows::None) => Some(true),
            (Borrows::One(lifetime), Borrows::One(output)) => {
                Some(outlives_lifetime(bounds, lifetime, output))
            }
            (Borrows::AtLeast(lifetime), Borrows::One(output))
                if outlives_lifetime(bounds, lifetime, output) =>
            {
                Some(true)
            }
            _ => None,
        }
    }
}

/// A function's own signature, as its body is checked against it: the
/// lifetimes of the borrows its receiver, each parameter and its output
/// hold, the output's elided ones given by elision.
pub(crate) struct Own {
    pub(crate) receiver: Borrows,
    pub(crate) parameters: Vec<Borrows>,
    pub(crate) output: Borrows,
}

/// `signature` as its function's body is checked against it.
pub(crate) fn own(signature: &Signature<'_>, declared: &dyn Fn(&Target) -> Option<usize>) -> Own {
    let read = read(signature, declared);
    let output: Vec<Region> = read
        .output
        .iter()
        .map(|region| match region {
            Region::Elided(_) => read.elided_to.clone().unwrap_or(Region::Unknown),
            region => region.clone(),
        })
        .collect();
    // A type parameter the output has is the same on both sides: its
    // regions are the caller's, whatever they are.
    let generic: Vec<&Region> = output
        .iter()
        .filter(|region| matches!(region, Region::Parameter(_)))
        .collect();
    let input = |shape: &Shape| {
        let regions = shape.all().filter(|region| !generic.contains(region));
        match (one(regions), &shape.outer) {
            (Borrows::Several, Some(outer)) => match one(std::iter::once(outer)) {
                Borrows::One(outer) => Borrows::AtLeast(outer),
                _ => Borrows::Several,
            },
            (borrows, _) => borrows,
        }
    };
    Own {
        receiver: read.receiver.as_ref().map_or(Borrows::None, input),
        parameters: read.parameters.iter().map(input).collect(),
        output: one(output
            .iter()
            .filter(|region| !matches!(region, Region::Parameter(_)))),
    }
}

/// The one lifetime `regions` all are.
fn one<'a>(regions: impl Iterator<Item = &'a Region>) -> Borrows {
    let mut found = None;
    for region in regions {
        let lifetime = match region {
            Region::Named(name) => Lifetime::Named(name.clone()),
            Region::Elided(number) => Lifetime::Elided(*number),
            Region::Static => Lifetime::Static,
            Region::Parameter(_) | Region::Captures | Region::Unknown => return Borrows::Several,
        };
        match &found {
            None => found = Some(lifetime),
            Some(one) if *one == lifetime => {}
            Some(_) => return Borrows::Several,
        }
    }
    found.map_or(Borrows::None, Borrows::One)
}

/// Whether `longer: shorter` by the signature's outlives `bounds`.
pub(crate) fn outlives_lifetime(
    bounds: &[(ImmutableString, ImmutableString)],
    longer: &Lifetime,
    shorter: &Lifetime,
) -> bool {
    match (longer, shorter) {
        (Lifetime::Static, _) => true,
        (longer, shorter) if longer == shorter => true,
        (Lifetime::Named(longer), Lifetime::Named(shorter)) => outlives(bounds, longer, shorter),
        (Lifetime::Named(longer), Lifetime::Static) => outlives(bounds, longer, "static"),
        _ => false,
    }
}

/// What a call of `signature` keeps of each of its inputs. `declared` says
/// how many lifetime parameters a crate type has.
pub(crate) fn summary(
    signature: &Signature<'_>,
    declared: &dyn Fn(&Target) -> Option<usize>,
) -> Summary {
    let Read {
        owner,
        receiver,
        parameters,
        output,
        elided_to,
        ..
    } = read(signature, declared);
    let bounds = &signature.lifetimes.bounds;
    let output: Vec<Region> = output
        .into_iter()
        .map(|region| match region {
            Region::Elided(_) => elided_to.clone().unwrap_or(Region::Unknown),
            region => region,
        })
        .collect();

    let receiver_keeps = receiver
        .as_ref()
        .map_or(Keeps::Nothing, |shape| keeps(shape, &output, bounds));
    let parameter_keeps = parameters
        .iter()
        .map(|shape| keeps(shape, &output, bounds))
        .collect();
    // A `&mut self` call can store its arguments' borrows in what `self`
    // holds.
    let stores = parameters
        .iter()
        .map(|shape| match signature.receiver {
            Some(Receiver::Mutable) => keeps(shape, &owner, bounds),
            _ => Keeps::Nothing,
        })
        .collect();
    let statics = parameters
        .iter()
        .map(|shape| {
            if shape.outer == Some(Region::Static) {
                return Keeps::All;
            }
            let bounded = |region: &Region| match region {
                Region::Static => true,
                Region::Parameter(parameter) => bounds
                    .iter()
                    .any(|(from, to)| from == parameter && to == "static"),
                _ => false,
            };
            if shape.inner.iter().any(bounded) {
                return if shape.outer.is_none() && shape.inner.iter().all(bounded) {
                    Keeps::All
                } else {
                    Keeps::Unknown
                };
            }
            Keeps::Nothing
        })
        .collect();
    // Another `&mut` input that can take a third's borrows.
    let unknown_stores = signature
        .parameters
        .iter()
        .zip(&parameters)
        .enumerate()
        .any(|(index, (ty, shape))| {
            matches!(ty, Type::Reference { mutable: true, .. })
                && !shape.inner.is_empty()
                && receiver
                    .iter()
                    .chain(
                        parameters
                            .iter()
                            .enumerate()
                            .filter(|(other, _)| *other != index)
                            .map(|(_, shape)| shape),
                    )
                    .any(|other| keeps(other, &shape.inner, bounds) != Keeps::Nothing)
        });
    Summary {
        mode: signature.receiver,
        receiver: receiver_keeps,
        parameters: parameter_keeps,
        stores,
        statics,
        unknown_stores,
    }
}
