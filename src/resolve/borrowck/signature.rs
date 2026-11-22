//! What a call's value keeps of what the call takes, read from the callee's
//! signature as rustc reads it (D572.4): lifetime elision, the lifetimes
//! written in it and the outlives bounds on them, `'static`, and its type
//! parameters.
//!
//! A call's value keeps its inputs' borrows region by region (T127): each
//! region position of the output takes the positions of the inputs whose
//! regions outlive it. A relation the signature does not say leaves the
//! call to rustc (D572.5).

use super::region::{Path, Step};
use crate::resolve::item::Lifetimes;
use crate::resolve::type_ref::{Argument, Target, Type};
use crate::{ImmutableString, Span};

/// How a method takes its receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Receiver {
    Shared,
    Mutable,
    Owned,
}

/// One input of a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    /// The receiver, as its method takes it: for `&self` the reference,
    /// with `Self` below `Arg(0)`; for `self` the value.
    Receiver,
    Argument(usize),
}

/// Where borrows a call keeps come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// What an input holds at a position.
    Input(Input, Path),
    /// What a call of the argument, a closure, returns.
    Call(usize),
}

/// A call keeps what `from` holds at `into`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tap {
    pub(crate) into: Path,
    pub(crate) from: Source,
}

/// What a call keeps of its inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    /// How the method takes its receiver.
    pub(crate) mode: Option<Receiver>,
    /// What the call's value holds, or `None` where the signature does not
    /// say.
    pub(crate) output: Option<Vec<Tap>>,
    /// What a `&mut self` call stores in what its receiver refers to
    /// (`v.push(&x)`), at positions of `Self`, or `None`.
    pub(crate) stores: Option<Vec<Tap>>,
    /// The inputs' positions borrowed for `'static`, or `None`.
    pub(crate) statics: Option<Vec<(Input, Path)>>,
    /// A `&mut` parameter other than the receiver may take another input's
    /// borrows, which these facts do not follow yet.
    pub(crate) unknown_stores: bool,
    /// The `&mut` arguments, by index, whose referents keep the receiver's
    /// borrow, as `scene.draw(&mut pass)` of `draw<'a>(&'a self, &mut
    /// RenderPass<'a>)` keeps `scene` borrowed in `pass`.
    pub(crate) receiver_into: Vec<usize>,
}

impl Summary {
    /// A call whose value keeps nothing it takes.
    pub(crate) fn nothing(mode: Option<Receiver>) -> Self {
        Self::taps(mode, Vec::new(), Vec::new())
    }

    /// A call whose value may keep any of what it takes.
    pub(crate) fn unknown() -> Self {
        Self {
            mode: None,
            output: None,
            stores: Some(Vec::new()),
            statics: Some(Vec::new()),
            unknown_stores: true,
            receiver_into: Vec::new(),
        }
    }

    /// A call taking its receiver as `mode` whose value keeps `output`, and
    /// which stores `stores` in its receiver.
    pub(crate) fn taps(mode: Option<Receiver>, output: Vec<Tap>, stores: Vec<Tap>) -> Self {
        Self {
            mode,
            output: Some(output),
            stores: Some(stores),
            statics: Some(Vec::new()),
            unknown_stores: false,
            receiver_into: Vec::new(),
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
    /// A type parameter: whatever regions its argument has, at the
    /// positions below its own.
    Parameter(ImmutableString),
    /// An `impl Trait` output, which captures every input's regions
    /// (edition 2024).
    Captures,
    /// A region the signature does not say (a trait object's, a function
    /// type's, a facade type's).
    Unknown,
}

/// A type's regions, each at its position.
type Positions = Vec<(Path, Region)>;

struct Reader<'a> {
    lifetimes: &'a Lifetimes,
    /// How many lifetime parameters a crate type declares, or `None` for one
    /// whose regions these facts can't place.
    declared: &'a dyn Fn(&Target) -> Option<usize>,
    /// Where a crate type is invariant.
    variance: Variance<'a>,
    /// Each elided lifetime, by its number less one.
    elided: Vec<Elision>,
    /// The positions read so far below which the type is invariant: what a
    /// `&mut` refers to, a cell's value, a crate type's invariant
    /// parameters.
    invariant: Vec<(Path, Invariance)>,
    /// The positions read so far below which the type is contravariant: a
    /// function type's arguments, a crate type's contravariant parameters.
    /// Below two of them it is covariant again.
    contravariant: Vec<Path>,
    /// The outlives bounds read so far that crate types imply between the
    /// lifetimes at two positions, longer first (RFC 2093).
    implied: Vec<(Path, Path)>,
    /// Whether a function type's argument and output types are read, below
    /// it, as a crate type's variance needs: a function value holds none of
    /// their borrows.
    functions: bool,
}

/// An elided lifetime of a signature: the type it is elided in, and
/// whether that type is a `&mut`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Elision {
    pub(crate) at: Span,
    pub(crate) mutable: bool,
    /// The `'_` written for it, or `None` where nothing is.
    pub(crate) placeholder: Option<Span>,
    /// It is a reference's: a `&` that rustc's suggestion of a named
    /// lifetime writes the lifetime after. A type whose lifetimes are left
    /// out is not.
    pub(crate) reference: bool,
    /// How many elided lifetimes its reference's type holds below it, each
    /// numbered after it.
    pub(crate) below: usize,
}

/// What makes a position of a type invariant, which rustc's notes on
/// variance name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Invariance {
    /// What a `&mut` refers to, of this type.
    Referent(Type),
    /// A type argument of this std cell or lock.
    Cell(Type),
    /// A lifetime or type argument of this crate type, at its position in
    /// the type ([`Step::Lifetime`] or [`Step::Arg`]).
    Nominal(Type, Path),
}

/// What a crate type's fields make of its lifetime and type parameters,
/// each a [`Step::Lifetime`] or [`Step::Arg`] path of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fields {
    /// Where its fields make it invariant: a parameter used in what a
    /// `&mut` refers to or in a cell's value, or used both ways.
    pub(crate) invariant: Vec<Path>,
    /// Where they make it contravariant: a parameter used only in function
    /// types' arguments.
    pub(crate) contravariant: Vec<Path>,
    /// The outlives bounds they imply between its lifetime parameters,
    /// longer first, by index (RFC 2093): `struct S<'a, 'b>(&'a &'b i32)`
    /// has `'b: 'a`.
    pub(crate) outlives: Vec<(u8, u8)>,
}

/// What a crate type's fields make of its parameters ([`Fields`]).
pub(crate) type Variance<'a> = &'a dyn Fn(&Target) -> Fields;

/// A crate type's variance where the signature's reading doesn't need it.
fn covariant(_: &Target) -> Fields {
    Fields::default()
}

/// How a position of a type varies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Varies {
    Covariant,
    Contravariant,
    Invariant,
}

impl Varies {
    /// The variance of a parameter used both ways.
    pub(crate) fn join(self, other: Self) -> Self {
        if self == other {
            self
        } else {
            Self::Invariant
        }
    }

    /// How the position `path` varies, below the `invariant` and
    /// `contravariant` positions of its type.
    fn at(invariant: &[(Path, Invariance)], contravariant: &[Path], path: Path) -> Self {
        if invariant.iter().any(|(root, _)| path.starts_with(*root)) {
            return Self::Invariant;
        }
        let flips = contravariant
            .iter()
            .filter(|root| path.starts_with(**root))
            .count();
        if flips % 2 == 1 {
            Self::Contravariant
        } else {
            Self::Covariant
        }
    }
}

/// Whether the builtin `name` is invariant in its type arguments, as std's
/// cells are.
fn invariant_builtin(name: &str) -> bool {
    matches!(
        name,
        "Cell" | "RefCell" | "UnsafeCell" | "OnceCell" | "Mutex" | "RwLock" | "OnceLock"
    )
}

impl Reader<'_> {
    /// The lifetime written at `span` in a type at `at`, or an elided one.
    fn lifetime(&mut self, span: Option<Span>, at: Span) -> Region {
        match span.and_then(|span| self.lifetimes.names.get(&span)) {
            Some(name) if name == "static" => Region::Static,
            Some(name) if name != "_" => Region::Named(name.clone()),
            Some(_) => {
                let fresh = self.fresh(at);
                if let Some(elision) = self.elided.last_mut() {
                    elision.placeholder = span;
                }
                fresh
            }
            None => self.fresh(at),
        }
    }

    /// An elided lifetime of the type at `at`.
    fn fresh(&mut self, at: Span) -> Region {
        self.elided.push(Elision {
            at,
            mutable: false,
            placeholder: None,
            reference: false,
            below: 0,
        });
        Region::Elided(self.elided.len())
    }

    /// The reference's lifetime `region`, read before what its type holds,
    /// as elided: its `&`'s, and the lifetimes elided below it since.
    fn referenced(&mut self, region: &Region) {
        if let Region::Elided(number) = region {
            let below = self.elided.len() - number;
            let elision = &mut self.elided[number - 1];
            elision.reference = true;
            elision.below = below;
        }
    }

    /// The regions of `ty`, at `at` and below. `output` when it is the
    /// call's value, whose structure the facts read back.
    fn positions(&mut self, ty: &Type, at: Path, output: bool, out: &mut Positions) {
        let own = at.push(Step::Own);
        match ty {
            Type::Unit(_) => {}
            Type::Tuple { members, .. } => {
                for (index, member) in members.iter().enumerate() {
                    self.positions(member, at.push(Step::Arg(index as u8)), output, out);
                }
            }
            Type::Reference {
                mutable,
                lifetime,
                target,
                span,
            } => {
                let region = self.lifetime(*lifetime, *span);
                if let Region::Elided(number) = region {
                    self.elided[number - 1].mutable = *mutable;
                }
                out.push((own, region.clone()));
                if *mutable {
                    self.invariant.push((
                        at.push(Step::Arg(0)),
                        Invariance::Referent((**target).clone()),
                    ));
                }
                self.positions(target, at.push(Step::Arg(0)), output, out);
                self.referenced(&region);
            }
            // `&str`, which a resolved type reads as `String` with the
            // reference's lifetime.
            Type::Path(path) if ty.is_str() => {
                let span = match &*path.arguments {
                    [Argument::Lifetime(span)] => Some(*span),
                    _ => None,
                };
                let region = self.lifetime(span, path.span);
                self.referenced(&region);
                out.push((own, region));
            }
            // `Cow<'a, B>` holds a borrow of its lifetime, of a `B` below
            // it: `Cow<'a, str>` is a `&'a str` or a `String`.
            Type::Path(path) if let Some(borrowed) = ty.cow_element() => {
                let lifetime = path.arguments.iter().find_map(|argument| match argument {
                    Argument::Lifetime(span) => Some(*span),
                    Argument::Type(_) => None,
                });
                let region = self.lifetime(lifetime, path.span);
                out.push((own, region));
                if !borrowed.is_str() {
                    self.positions(borrowed, at.push(Step::Arg(0)), output, out);
                }
            }
            // An iterator's value captures what it was made of; as an
            // output, its items are below it.
            Type::Path(_) if ty.iterator_item().is_some() => {
                out.push((own, Region::Captures));
                if output && let Some((item, _)) = ty.iterator_item() {
                    self.positions(item, at.push(Step::Arg(0)), output, out);
                }
            }
            // A future captures what it was made of, as an `impl Trait`
            // output does: an `async fn`'s future holds each borrow its
            // inputs hold for as long as it lives. As an output, what it
            // gives is below it.
            Type::Path(_) if let Some(output_ty) = ty.future_output() => {
                out.push((own, Region::Captures));
                if output {
                    self.positions(output_ty, at.push(Step::Arg(0)), output, out);
                }
            }
            // A function value holds what it captures: within the
            // lifetimes its type is bounded by (`impl Fn() + 'a`), or any.
            Type::Path(path) if let Some((arguments, returned)) = ty.fn_signature() => {
                if self.functions {
                    for (index, argument) in arguments.iter().enumerate() {
                        let below = at.push(Step::Arg(index as u8));
                        self.contravariant.push(below);
                        self.positions(argument, below, output, out);
                    }
                    let below = at.push(Step::Arg(arguments.len() as u8));
                    self.positions(returned, below, output, out);
                }
                let bounded: Vec<Region> = path
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Argument::Lifetime(span) => Some(self.lifetime(Some(*span), *span)),
                        Argument::Type(_) => None,
                    })
                    .collect();
                if bounded.is_empty() {
                    out.push((own, Region::Unknown));
                }
                out.extend(bounded.into_iter().map(|region| (own, region)));
            }
            Type::Path(_) if ty.dyn_trait().is_some() => out.push((own, Region::Unknown)),
            Type::Path(path) => {
                match &path.target {
                    Target::Parameter if path.name == "Self" => match &self.lifetimes.owner {
                        Some(owner) => {
                            let owner = owner.clone();
                            self.positions(&owner, at, output, out);
                        }
                        None => out.push((own, Region::Unknown)),
                    },
                    Target::Parameter | Target::Opaque(_) => {
                        out.push((at, Region::Parameter(path.name.clone())))
                    }
                    Target::Projection(..)
                    | Target::Alias(_)
                    | Target::Std(_)
                    | Target::Host { .. } => {
                        out.push((own, Region::Unknown));
                    }
                    Target::Builtin if opaque(&path.name) => out.push((own, Region::Captures)),
                    Target::Builtin | Target::Script { .. } => {}
                }
                let script = matches!(path.target, Target::Script { .. });
                if script {
                    let fields = (self.variance)(&path.target);
                    self.invariant.extend(
                        fields
                            .invariant
                            .into_iter()
                            .map(|below| (at.join(below), Invariance::Nominal(ty.clone(), below))),
                    );
                    self.contravariant
                        .extend(fields.contravariant.into_iter().map(|below| at.join(below)));
                    self.implied
                        .extend(fields.outlives.into_iter().map(|(longer, shorter)| {
                            (
                                at.push(Step::Lifetime(longer)),
                                at.push(Step::Lifetime(shorter)),
                            )
                        }));
                }
                let cell =
                    (path.target == Target::Builtin && invariant_builtin(&path.name)).then_some(ty);
                let mut written = 0;
                let mut types = 0;
                for argument in &path.arguments {
                    match argument {
                        Argument::Type(ty) => {
                            if let Some(cell) = cell {
                                self.invariant.push((
                                    at.push(Step::Arg(types)),
                                    Invariance::Cell(cell.clone()),
                                ));
                            }
                            self.positions(ty, at.push(Step::Arg(types)), output, out);
                            types += 1;
                        }
                        // A builtin's lifetime has no place in its value.
                        Argument::Lifetime(_) if !script => out.push((own, Region::Unknown)),
                        Argument::Lifetime(span) => {
                            let region = self.lifetime(Some(*span), *span);
                            out.push((at.push(Step::Lifetime(written)), region));
                            written += 1;
                        }
                    }
                }
                // `Wrapper` of `struct Wrapper<'a>`: its lifetimes elided.
                if script {
                    match (self.declared)(&path.target) {
                        Some(declared) => {
                            for index in usize::from(written)..declared {
                                let fresh = self.fresh(path.span);
                                out.push((at.push(Step::Lifetime(index as u8)), fresh));
                            }
                        }
                        None => out.push((own, Region::Unknown)),
                    }
                }
            }
        }
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

/// Whether a region of an input must outlive `into`: whether what it
/// borrows is kept there. `None` when the signature does not say.
fn relates(
    region: &Region,
    into: &Region,
    bounds: &[(ImmutableString, ImmutableString)],
) -> Option<bool> {
    match (region, into) {
        (_, Region::Static) | (Region::Static, _) => Some(false),
        (_, Region::Captures) => Some(true),
        (_, Region::Unknown) | (Region::Unknown, _) => None,
        (Region::Named(longer), Region::Named(shorter)) => Some(outlives(bounds, longer, shorter)),
        (Region::Elided(a), Region::Elided(b)) => Some(a == b),
        (Region::Parameter(a), Region::Parameter(b)) => Some(a == b),
        // `T: 'a` keeps `T`'s borrows in `'a`.
        (Region::Parameter(parameter), Region::Named(shorter)) => Some(
            bounds
                .iter()
                .any(|(from, to)| from == parameter && outlives(bounds, to, shorter)),
        ),
        _ => Some(false),
    }
}

/// The taps that keep at `into` (of region `region`) what `inputs` hold of
/// the regions that outlive it, or `None` when the signature does not say.
fn kept(
    into: Path,
    region: &Region,
    inputs: &[(Input, Positions)],
    bounds: &[(ImmutableString, ImmutableString)],
    taps: &mut Vec<Tap>,
) -> Option<()> {
    match region {
        Region::Static => return Some(()),
        Region::Unknown => return None,
        _ => {}
    }
    for (input, positions) in inputs {
        for (from, input_region) in positions {
            if !relates(input_region, region, bounds)? {
                continue;
            }
            // A type parameter's value kept in a region of another kind is
            // all of it there.
            let from = match (input_region, region) {
                (Region::Parameter(_), Region::Parameter(_)) => *from,
                (Region::Parameter(_), _) => from.push(Step::Own),
                _ => *from,
            };
            taps.push(Tap {
                into,
                from: Source::Input(*input, from),
            });
        }
    }
    Some(())
}

/// A signature's regions, as elision gives them.
struct Read {
    /// The impl's owner's, which `self` is.
    owner: Positions,
    /// The receiver's and each parameter's.
    inputs: Vec<(Input, Positions)>,
    /// The output's, elided ones not yet given an input's.
    output: Positions,
    /// What elision gives the output's elided lifetimes: `&self`'s, or the
    /// one input lifetime. `None` when it gives none, or when the inputs
    /// have regions the signature does not say.
    elided_to: Option<Region>,
    /// The inputs have regions the signature does not say.
    unknown: bool,
    /// Each elided lifetime, by its number less one.
    elided: Vec<Elision>,
    /// How many of them the inputs elide: the output's follow.
    input_elided: usize,
    /// The output's positions below which it is invariant.
    invariant: Vec<(Path, Invariance)>,
    /// The output's positions below which it is contravariant.
    contravariant: Vec<Path>,
    /// The bounds crate types imply between two positions of each input,
    /// as [`Read::inputs`] has them, and of the output.
    implied: Vec<Vec<(Path, Path)>>,
    output_implied: Vec<(Path, Path)>,
    /// Where each input, as [`Read::inputs`] has them, varies.
    varied: Vec<Varied>,
}

/// Where a type is invariant, and what makes each position so, and where
/// it is contravariant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Varied {
    invariant: Vec<(Path, Invariance)>,
    contravariant: Vec<Path>,
}

fn read(
    signature: &Signature<'_>,
    declared: &dyn Fn(&Target) -> Option<usize>,
    variance: Variance<'_>,
) -> Read {
    let mut reader = Reader {
        lifetimes: signature.lifetimes,
        declared,
        variance,
        elided: Vec::new(),
        invariant: Vec::new(),
        contravariant: Vec::new(),
        implied: Vec::new(),
        functions: false,
    };
    let mut owner = Positions::new();
    match &signature.lifetimes.owner {
        Some(ty) => reader.positions(ty, Path::ROOT, false, &mut owner),
        None if signature.receiver.is_some() => owner.push((Path::OWN, Region::Unknown)),
        None => {}
    }
    let owner_implied = std::mem::take(&mut reader.implied);
    let owner_invariant = std::mem::take(&mut reader.invariant);
    let owner_contravariant = std::mem::take(&mut reader.contravariant);
    let mut inputs: Vec<(Input, Positions)> = Vec::new();
    let mut implied = Vec::new();
    let mut varied = Vec::new();
    // The receiver's own region, which elided output lifetimes take.
    let mut receiver_region = None;
    if let Some(mode) = signature.receiver {
        let positions =
            match mode {
                Receiver::Owned => owner.clone(),
                Receiver::Shared | Receiver::Mutable => {
                    let region = match &signature.lifetimes.receiver {
                        Some(name) if name == "static" => Region::Static,
                        Some(name) if name != "_" => Region::Named(name.clone()),
                        _ => {
                            let fresh = reader.fresh(Span::NONE);
                            if let Some(elision) = reader.elided.last_mut() {
                                elision.mutable = mode == Receiver::Mutable;
                                elision.reference = signature.lifetimes.receiver.is_none();
                            }
                            fresh
                        }
                    };
                    receiver_region = Some(region.clone());
                    let mut positions = vec![(Path::OWN, region)];
                    positions.extend(owner.iter().map(|(path, region)| {
                        (Path::of(&[Step::Arg(0)]).join(*path), region.clone())
                    }));
                    positions
                }
            };
        let below = match mode {
            Receiver::Owned => Path::ROOT,
            Receiver::Shared | Receiver::Mutable => Path::of(&[Step::Arg(0)]),
        };
        implied.push(
            owner_implied
                .iter()
                .map(|&(longer, shorter)| (below.join(longer), below.join(shorter)))
                .collect(),
        );
        let mut invariant: Vec<(Path, Invariance)> = owner_invariant
            .iter()
            .map(|(path, invariance)| (below.join(*path), invariance.clone()))
            .collect();
        if mode == Receiver::Mutable
            && let Some(owner) = &signature.lifetimes.owner
        {
            invariant.push((below, Invariance::Referent((**owner).clone())));
        }
        let contravariant = owner_contravariant
            .iter()
            .map(|path| below.join(*path))
            .collect();
        varied.push(Varied {
            invariant,
            contravariant,
        });
        inputs.push((Input::Receiver, positions));
    }
    let written = signature.lifetimes.written_parameters.as_deref();
    for (index, ty) in signature.parameters.iter().enumerate() {
        let ty = written.and_then(|written| written.get(index)).unwrap_or(ty);
        let mut positions = Positions::new();
        reader.positions(ty, Path::ROOT, false, &mut positions);
        implied.push(std::mem::take(&mut reader.implied));
        varied.push(Varied {
            invariant: std::mem::take(&mut reader.invariant),
            contravariant: std::mem::take(&mut reader.contravariant),
        });
        inputs.push((Input::Argument(index), positions));
    }
    let mut output = Positions::new();
    let written = signature.lifetimes.written_output.as_deref();
    let input_elided = reader.elided.len();
    reader.invariant.clear();
    reader.contravariant.clear();
    reader.positions(
        written.unwrap_or(signature.output),
        Path::ROOT,
        true,
        &mut output,
    );

    let mut distinct: Vec<&Region> = Vec::new();
    let mut unknown = false;
    for (_, region) in inputs.iter().flat_map(|(_, positions)| positions) {
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
    // Elided output lifetimes are `&self`'s, or the one input lifetime.
    let elided_to = receiver_region.or_else(|| match distinct[..] {
        [only] if !unknown => Some(only.clone()),
        _ => None,
    });
    Read {
        owner,
        inputs,
        output,
        elided_to,
        unknown,
        elided: reader.elided,
        input_elided,
        invariant: reader.invariant,
        contravariant: reader.contravariant,
        implied,
        output_implied: reader.implied,
        varied,
    }
}

/// Where the output of `signature` has a lifetime elided that elision
/// gives no input lifetime to: rustc's E0106. `None` when there is none, or
/// when the inputs have regions the signature does not say.
pub(crate) fn missing_lifetime(
    signature: &Signature<'_>,
    declared: &dyn Fn(&Target) -> Option<usize>,
) -> Option<Span> {
    let read = read(signature, declared, &covariant);
    if read.elided_to.is_some() || read.unknown {
        return None;
    }
    read.output.iter().find_map(|(_, region)| match region {
        Region::Elided(number) => read.elided.get(number - 1).map(|elision| elision.at),
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

impl Lifetime {
    /// The lifetime `region` is, or `None` for one the signature does not
    /// say.
    fn of(region: &Region) -> Option<Self> {
        match region {
            Region::Named(name) => Some(Lifetime::Named(name.clone())),
            Region::Elided(number) => Some(Lifetime::Elided(*number)),
            Region::Static => Some(Lifetime::Static),
            Region::Parameter(_) | Region::Captures | Region::Unknown => None,
        }
    }

    /// The lifetime a bound names.
    fn named(name: &ImmutableString) -> Self {
        if name == "static" {
            Lifetime::Static
        } else {
            Lifetime::Named(name.clone())
        }
    }
}

/// The lifetime at each region position of an input or the output of a
/// function's own signature, or `None` at one the signature does not say:
/// a trait object's or an `impl Trait`'s. A type parameter's positions
/// are left out: what it holds is the caller's on both sides.
pub(crate) type Regions = Vec<(Path, Option<Lifetime>)>;

/// What outlives what in a function's own signature: the bounds it writes,
/// and the ones its types imply, as `&'a &'b T` has `'b: 'a`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bounds(Vec<(Lifetime, Lifetime)>);

impl Bounds {
    /// The bounds `written`, and those the types of `regions` imply: each
    /// region below a value's own region outlives it, and each crate type
    /// implies its fields' bounds, at the pairs of positions of each.
    fn of<'a>(
        written: &[(ImmutableString, ImmutableString)],
        regions: impl IntoIterator<Item = (&'a Regions, &'a [(Path, Path)])>,
    ) -> Self {
        let mut bounds: Vec<(Lifetime, Lifetime)> = written
            .iter()
            .map(|(longer, shorter)| (Lifetime::named(longer), Lifetime::named(shorter)))
            .collect();
        for (regions, implied) in regions {
            let at = |position: &Path| {
                regions
                    .iter()
                    .find(|(path, _)| path == position)
                    .and_then(|(_, lifetime)| lifetime.clone())
            };
            for (longer, shorter) in implied {
                if let (Some(longer), Some(shorter)) = (at(longer), at(shorter)) {
                    bounds.push((longer, shorter));
                }
            }
            for (own, shorter) in regions {
                let (true, Some(shorter)) = (own.is_own(), shorter) else {
                    continue;
                };
                let value = own.parent();
                for (below, longer) in regions {
                    if let Some(longer) = longer
                        && below != own
                        && below.starts_with(value)
                    {
                        bounds.push((longer.clone(), shorter.clone()));
                    }
                }
            }
        }
        Self(bounds)
    }

    /// Whether `longer: shorter`.
    pub(crate) fn outlives(&self, longer: &Lifetime, shorter: &Lifetime) -> bool {
        let mut seen = vec![longer];
        let mut index = 0;
        while let Some(&at) = seen.get(index) {
            if at == shorter || *at == Lifetime::Static {
                return true;
            }
            for (from, to) in &self.0 {
                if from == at && !seen.contains(&to) {
                    seen.push(to);
                }
            }
            index += 1;
        }
        false
    }
}

/// A function's own signature, as its body is checked against it (T170):
/// the regions of its receiver, each parameter and its output.
pub(crate) struct Own {
    pub(crate) receiver: Parameter,
    pub(crate) parameters: Vec<Parameter>,
    pub(crate) output: Output,
    /// Each elided lifetime, by its number less one: the receiver's is at
    /// no place.
    pub(crate) elided: Vec<Elision>,
    /// How many of them the inputs elide: the output's follow.
    pub(crate) input_elided: usize,
}

/// A receiver or a parameter of a function's own signature, as its body
/// is checked against it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Parameter {
    pub(crate) regions: Regions,
    /// Its positions below which it is invariant, and what makes each so.
    invariant: Vec<(Path, Invariance)>,
    /// Its positions below which it is contravariant.
    contravariant: Vec<Path>,
}

impl Parameter {
    /// What makes the value at its position `at` invariant, where a `&mut`
    /// or a cell holds it.
    pub(crate) fn invariance_at(&self, at: Path) -> Option<&Invariance> {
        self.invariant
            .iter()
            .find(|(root, _)| *root == at)
            .map(|(_, invariance)| invariance)
    }

    /// What makes each of its invariant positions so.
    pub(crate) fn invariances(&self) -> impl Iterator<Item = &Invariance> {
        self.invariant.iter().map(|(_, invariance)| invariance)
    }
}

/// A function's output, as what its body returns is checked against it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Output {
    /// Its regions, the elided ones given by elision.
    regions: Regions,
    /// Its positions below which it is invariant, and what makes each so:
    /// what is returned there has its lifetimes exactly.
    invariant: Vec<(Path, Invariance)>,
    /// Its positions below which it is contravariant: what is returned
    /// there has lifetimes its lifetimes outlive.
    contravariant: Vec<Path>,
    bounds: Bounds,
}

/// `signature` as its function's body is checked against it. `variance`
/// says where a crate type is invariant.
pub(crate) fn own(
    signature: &Signature<'_>,
    declared: &dyn Fn(&Target) -> Option<usize>,
    variance: Variance<'_>,
) -> Own {
    let read = read(signature, declared, variance);
    // A type parameter is the same type wherever the signature names it:
    // what it holds is the caller's on both sides.
    let generic = |(_, region): &&(Path, Region)| !matches!(region, Region::Parameter(_));
    let output: Regions = read
        .output
        .iter()
        .filter(generic)
        .map(|(path, region)| {
            let lifetime = match region {
                Region::Elided(_) => read.elided_to.as_ref().and_then(Lifetime::of),
                region => Lifetime::of(region),
            };
            (*path, lifetime)
        })
        .collect();
    let mut receiver = Parameter::default();
    let mut parameters = Vec::new();
    for ((input, positions), varied) in read.inputs.iter().zip(&read.varied) {
        let parameter = Parameter {
            regions: positions
                .iter()
                .filter(generic)
                .map(|(path, region)| (*path, Lifetime::of(region)))
                .collect(),
            invariant: varied.invariant.clone(),
            contravariant: varied.contravariant.clone(),
        };
        match input {
            Input::Receiver => receiver = parameter,
            Input::Argument(_) => parameters.push(parameter),
        }
    }
    let has_receiver = read
        .inputs
        .first()
        .is_some_and(|(input, _)| *input == Input::Receiver);
    let mut implied = read.implied.iter().map(Vec::as_slice);
    let receiver_implied = if has_receiver {
        implied.next().unwrap_or_default()
    } else {
        &[]
    };
    let bounds = Bounds::of(
        &signature.lifetimes.bounds,
        std::iter::once((&receiver.regions, receiver_implied))
            .chain(
                parameters
                    .iter()
                    .map(|parameter| &parameter.regions)
                    .zip(implied),
            )
            .chain(std::iter::once((&output, &read.output_implied[..]))),
    );
    Own {
        receiver,
        parameters,
        output: Output {
            regions: output,
            invariant: read.invariant,
            contravariant: read.contravariant,
            bounds,
        },
        elided: read.elided,
        input_elided: read.input_elided,
    }
}

/// How an input's lifetime that the body returns at a position of the
/// output relates to the output's lifetimes there ([`Output::relate`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relation<'a> {
    /// As the output's type requires.
    Holds,
    /// Not as the output's type requires.
    Broken(Required<'a>),
    /// These facts can't tell: a lifetime the signature does not say, or
    /// several the value reaches without telling their positions apart.
    Unsaid,
}

/// What the output's type requires of an input's lifetime that the body
/// returns in it, and the body breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Required<'a> {
    /// The output's lifetime at the position.
    pub(crate) output: &'a Lifetime,
    /// Which must outlive which.
    pub(crate) direction: Direction,
    /// What makes the position invariant, where it is.
    pub(crate) invariance: Option<&'a Invariance>,
}

/// Which of an input's lifetime and the output's must outlive the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    /// The input's must outlive the output's.
    Input,
    /// The output's must outlive the input's, at an invariant position.
    Output,
}

impl Output {
    /// A value below a reference in `parameter`, whose positions are at
    /// `positions` of the parameter's type by prefix, as what the body
    /// stores there is checked against it: the parameter's regions there,
    /// and where the value's type varies below its root, with the
    /// signature's bounds. Where a field's positions are not one prefix of
    /// the parameter's, what makes its type vary is its owner's.
    pub(crate) fn stored(&self, parameter: &Parameter, positions: &[(Path, Path)]) -> Self {
        let value = |path: &Path| {
            positions
                .iter()
                .filter(|(_, local)| path.starts_with(*local))
                .max_by_key(|(_, local)| local.len())
                .map(|(value, local)| value.join(path.strip(*local)))
        };
        let whole = matches!(positions, [(root, _)] if root.is_root());
        let below = |path: &Path| value(path).filter(|path| whole && !path.is_root());
        Self {
            regions: parameter
                .regions
                .iter()
                .filter_map(|(path, lifetime)| Some((value(path)?, lifetime.clone())))
                .collect(),
            invariant: parameter
                .invariant
                .iter()
                .filter_map(|(path, invariance)| Some((below(path)?, invariance.clone())))
                .collect(),
            contravariant: parameter.contravariant.iter().filter_map(below).collect(),
            bounds: self.bounds.clone(),
        }
    }

    /// What makes each of its invariant positions so.
    pub(crate) fn invariances(&self) -> impl Iterator<Item = &Invariance> {
        self.invariant.iter().map(|(_, invariance)| invariance)
    }

    /// How `lifetime`, which an input holds and the body returns at the
    /// output's position `at`, relates to the output's lifetimes there. It
    /// must outlive each lifetime at or below `at`, and be the one at an
    /// invariant position. Where it is neither, rustc reports the output's
    /// first.
    pub(crate) fn relate(&self, lifetime: &Lifetime, at: Path) -> Relation<'_> {
        match self.broken(lifetime, at) {
            None => Relation::Unsaid,
            Some(broken) => broken
                .first()
                .copied()
                .map_or(Relation::Holds, Relation::Broken),
        }
    }

    /// Each requirement of [`Output::relate`] the body breaks, in the
    /// order rustc reports them, or `None` where these facts can't tell.
    pub(crate) fn broken(&self, lifetime: &Lifetime, at: Path) -> Option<Vec<Required<'_>>> {
        let at = self.within(at);
        let mut below: Vec<(Path, &Lifetime)> = Vec::new();
        for (path, output) in &self.regions {
            match output {
                _ if !path.starts_with(at) => {}
                Some(output) => below.push((*path, output)),
                None => return None,
            }
        }
        let exact = matches!(below[..], [(path, _)] if path == at);
        if !exact && below.iter().any(|(_, output)| *output != below[0].1) {
            return None;
        }
        let invariance = |path: Path| {
            self.invariant
                .iter()
                .find(|(root, _)| exact && path.starts_with(*root))
                .map(|(_, invariance)| invariance)
        };
        let contravariant = |path: Path| {
            exact && Varies::at(&[], &self.contravariant, path) == Varies::Contravariant
        };
        let mut broken = Vec::new();
        for (path, output) in below {
            let invariance = invariance(path);
            // The input's lifetime must outlive the output's at a
            // covariant position, be outlived by it at a contravariant
            // one, and both at an invariant one.
            let varies = if invariance.is_some() {
                Varies::Invariant
            } else if contravariant(path) {
                Varies::Contravariant
            } else {
                Varies::Covariant
            };
            let required = |direction| Required {
                output,
                direction,
                invariance,
            };
            if varies != Varies::Covariant && !self.bounds.outlives(output, lifetime) {
                broken.push(required(Direction::Output));
            }
            if varies != Varies::Contravariant && !self.bounds.outlives(lifetime, output) {
                broken.push(required(Direction::Input));
            }
        }
        Some(broken)
    }

    /// `at`, or the own region that holds it where the output's type has
    /// no region at or below `at`: what a reborrow keeps below the
    /// reference it makes is within that reference's region, as what
    /// `&x[1..]` of a `&'a str` keeps is within `'a`.
    fn within(&self, at: Path) -> Path {
        let mut above = at;
        while !self.regions.iter().any(|(path, _)| path.starts_with(above)) {
            if above.is_root() {
                return at;
            }
            let value = above.parent();
            let own = if value.is_own() {
                value
            } else {
                value.push(Step::Own)
            };
            if self.regions.iter().any(|(path, _)| *path == own) {
                return own;
            }
            above = value;
        }
        at
    }
}

/// What the field or payload of type `ty` of a crate type makes of the
/// type's parameters ([`Fields`]): how each parameter it uses varies there,
/// by its [`Step::Lifetime`] or [`Step::Arg`] path, and the outlives bounds
/// it implies between the type's lifetime parameters. Its lifetime
/// parameters are `regions`' and its type parameters `parameters`.
pub(crate) fn field_variance(
    ty: &Type,
    regions: &crate::resolve::item::TypeRegions,
    parameters: &[ImmutableString],
    declared: &dyn Fn(&Target) -> Option<usize>,
    variance: Variance<'_>,
) -> FieldVariance {
    let parameter = |region: &Region| match region {
        Region::Named(name) => regions
            .params
            .iter()
            .position(|param| param == name)
            .map(|index| Step::Lifetime(index as u8)),
        Region::Parameter(name) => parameters
            .iter()
            .position(|param| param == name)
            .map(|index| Step::Arg(index as u8)),
        _ => None,
    };
    let reader = |functions| Reader {
        lifetimes: &regions.written,
        declared,
        variance,
        elided: Vec::new(),
        invariant: Vec::new(),
        contravariant: Vec::new(),
        implied: Vec::new(),
        functions,
    };
    let mut varied = reader(true);
    let mut positions = Positions::new();
    varied.positions(ty, Path::ROOT, true, &mut positions);
    let mut varies: Vec<(Path, Varies)> = Vec::new();
    for (path, region) in &positions {
        let Some(step) = parameter(region) else {
            continue;
        };
        let at = Varies::at(&varied.invariant, &varied.contravariant, *path);
        let step = Path::of(&[step]);
        match varies.iter_mut().find(|(known, _)| *known == step) {
            Some((_, known)) => *known = known.join(at),
            None => varies.push((step, at)),
        }
    }
    // A function type's argument and output types imply nothing of the
    // type's lifetimes: a function value holds none of their borrows.
    let mut held = reader(false);
    let mut positions = Positions::new();
    held.positions(ty, Path::ROOT, true, &mut positions);
    let lifetime = |region: &Region| match parameter(region) {
        Some(Step::Lifetime(index)) => Some(index),
        _ => None,
    };
    let mut outlives = Vec::new();
    for (own, shorter) in &positions {
        let (true, Some(shorter)) = (own.is_own(), lifetime(shorter)) else {
            continue;
        };
        for (below, longer) in &positions {
            if let Some(longer) = lifetime(longer)
                && below != own
                && below.starts_with(own.parent())
                && longer != shorter
                && !outlives.contains(&(longer, shorter))
            {
                outlives.push((longer, shorter));
            }
        }
    }
    for (longer, shorter) in &held.implied {
        let at = |position: &Path| {
            positions
                .iter()
                .find(|(path, _)| path == position)
                .and_then(|(_, region)| lifetime(region))
        };
        if let (Some(longer), Some(shorter)) = (at(longer), at(shorter))
            && longer != shorter
            && !outlives.contains(&(longer, shorter))
        {
            outlives.push((longer, shorter));
        }
    }
    FieldVariance { varies, outlives }
}

/// What a field of a crate type makes of the type's parameters
/// ([`field_variance`]).
pub(crate) struct FieldVariance {
    /// How each parameter it uses varies there, by its path.
    pub(crate) varies: Vec<(Path, Varies)>,
    /// The bounds it implies between the type's lifetime parameters,
    /// longer first, by index.
    pub(crate) outlives: Vec<(u8, u8)>,
}

/// What a call of `signature` keeps of each of its inputs. `declared` says
/// how many lifetime parameters a crate type has.
pub(crate) fn summary(
    signature: &Signature<'_>,
    declared: &dyn Fn(&Target) -> Option<usize>,
) -> Summary {
    let Read {
        owner,
        inputs,
        output,
        elided_to,
        ..
    } = read(signature, declared, &covariant);
    let bounds = &signature.lifetimes.bounds;
    let mut taps = Vec::new();
    let known = output.into_iter().all(|(into, region)| {
        let region = match region {
            Region::Elided(_) => elided_to.clone().unwrap_or(Region::Unknown),
            region => region,
        };
        kept(into, &region, &inputs, bounds, &mut taps).is_some()
    });
    let arguments: Vec<(Input, Positions)> = inputs
        .iter()
        .filter(|(input, _)| *input != Input::Receiver)
        .cloned()
        .collect();
    // A `&mut self` call can store its arguments' borrows in what `self`
    // holds.
    let stores = match signature.receiver {
        Some(Receiver::Mutable) => {
            let mut stores = Vec::new();
            owner
                .iter()
                .all(|(into, region)| {
                    kept(*into, region, &arguments, bounds, &mut stores).is_some()
                })
                .then_some(stores)
        }
        _ => Some(Vec::new()),
    };
    let statics = arguments
        .iter()
        .flat_map(|(input, positions)| {
            positions.iter().filter_map(move |(path, region)| {
                let forever = match region {
                    Region::Static => true,
                    Region::Parameter(parameter) => bounds
                        .iter()
                        .any(|(from, to)| from == parameter && to == "static"),
                    _ => false,
                };
                forever.then_some((*input, *path))
            })
        })
        .collect();
    // Another `&mut` input that can take a third's borrows.
    let unknown_stores = signature.parameters.iter().enumerate().any(|(index, ty)| {
        if !matches!(ty, Type::Reference { mutable: true, .. }) {
            return false;
        }
        let below = Path::of(&[Step::Arg(0)]);
        let inner: Vec<&Region> = inputs
            .iter()
            .filter(|(input, _)| *input == Input::Argument(index))
            .flat_map(|(_, positions)| positions)
            .filter(|(path, _)| path.starts_with(below))
            .map(|(_, region)| region)
            .collect();
        inputs
            .iter()
            .filter(|(input, _)| *input != Input::Argument(index))
            .flat_map(|(_, positions)| positions)
            .any(|(_, region)| {
                inner
                    .iter()
                    .any(|into| relates(region, into, bounds) != Some(false))
            })
    });
    Summary {
        mode: signature.receiver,
        output: known.then_some(taps),
        stores,
        statics: Some(statics),
        unknown_stores,
        receiver_into: Vec::new(),
    }
}

/// Where each region position of a field's type `ty` sits in the crate type
/// that declares it (T127): the field's position and the type's, or `None`
/// for a `'static` position. The type's lifetime parameters are `regions`'
/// and its type parameters `parameters`. `None` when a position is neither,
/// or the type bounds its lifetimes, which these facts don't read.
pub(crate) fn field_positions(
    ty: &Type,
    regions: &crate::resolve::item::TypeRegions,
    parameters: &[ImmutableString],
    declared: &dyn Fn(&Target) -> Option<usize>,
) -> Option<Vec<(Path, Option<Path>)>> {
    if regions.bounded {
        return None;
    }
    let mut reader = Reader {
        lifetimes: &regions.written,
        declared,
        variance: &covariant,
        elided: Vec::new(),
        invariant: Vec::new(),
        contravariant: Vec::new(),
        implied: Vec::new(),
        functions: false,
    };
    let mut positions = Positions::new();
    reader.positions(ty, Path::ROOT, true, &mut positions);
    positions
        .into_iter()
        .map(|(path, region)| match region {
            Region::Static => Some((path, None)),
            Region::Named(name) => {
                let index = regions.params.iter().position(|param| *param == name)?;
                Some((path, Some(Path::of(&[Step::Lifetime(index as u8)]))))
            }
            Region::Parameter(name) => {
                let index = parameters.iter().position(|param| *param == name)?;
                Some((path, Some(Path::of(&[Step::Arg(index as u8)]))))
            }
            Region::Elided(_) | Region::Captures | Region::Unknown => None,
        })
        .collect()
}

/// A callee's signature, as the borrow checker reads it.
pub(crate) struct Signature<'a> {
    pub(crate) receiver: Option<Receiver>,
    pub(crate) parameters: Vec<&'a Type>,
    pub(crate) output: &'a Type,
    pub(crate) lifetimes: &'a Lifetimes,
}
