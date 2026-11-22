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
}

/// An elided lifetime of a signature: the type it is elided in, and
/// whether that type is a `&mut`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Elision {
    pub(crate) at: Span,
    pub(crate) mutable: bool,
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

/// The positions of a crate type, each a [`Step::Lifetime`] or
/// [`Step::Arg`] of it, at which its fields make it invariant: a lifetime
/// or type parameter used in what a `&mut` refers to or in a cell's value.
pub(crate) type Variance<'a> = &'a dyn Fn(&Target) -> Vec<Path>;

/// A crate type's variance where the signature's reading doesn't need it.
fn covariant(_: &Target) -> Vec<Path> {
    Vec::new()
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
            _ => self.fresh(at),
        }
    }

    /// An elided lifetime of the type at `at`.
    fn fresh(&mut self, at: Span) -> Region {
        self.elided.push(Elision { at, mutable: false });
        Region::Elided(self.elided.len())
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
                out.push((own, region));
                if *mutable {
                    self.invariant.push((
                        at.push(Step::Arg(0)),
                        Invariance::Referent((**target).clone()),
                    ));
                }
                self.positions(target, at.push(Step::Arg(0)), output, out);
            }
            // `&str`, which a resolved type reads as `String` with the
            // reference's lifetime.
            Type::Path(path) if ty.is_str() => {
                let span = match &*path.arguments {
                    [Argument::Lifetime(span)] => Some(*span),
                    _ => None,
                };
                let region = self.lifetime(span, path.span);
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
            Type::Path(path) if ty.fn_signature().is_some() => {
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
                    Target::Parameter => out.push((at, Region::Parameter(path.name.clone()))),
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
                    let invariant = (self.variance)(&path.target);
                    self.invariant.extend(
                        invariant
                            .into_iter()
                            .map(|below| (at.join(below), Invariance::Nominal(ty.clone(), below))),
                    );
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
    /// The output's positions below which it is invariant.
    invariant: Vec<(Path, Invariance)>,
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
    };
    let mut owner = Positions::new();
    match &signature.lifetimes.owner {
        Some(ty) => reader.positions(ty, Path::ROOT, false, &mut owner),
        None if signature.receiver.is_some() => owner.push((Path::OWN, Region::Unknown)),
        None => {}
    }
    let mut inputs: Vec<(Input, Positions)> = Vec::new();
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
        inputs.push((Input::Receiver, positions));
    }
    let written = signature.lifetimes.written_parameters.as_deref();
    for (index, ty) in signature.parameters.iter().enumerate() {
        let ty = written.and_then(|written| written.get(index)).unwrap_or(ty);
        let mut positions = Positions::new();
        reader.positions(ty, Path::ROOT, false, &mut positions);
        inputs.push((Input::Argument(index), positions));
    }
    let mut output = Positions::new();
    let written = signature.lifetimes.written_output.as_deref();
    reader.invariant.clear();
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
        invariant: reader.invariant,
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
    /// region below a value's own region outlives it.
    fn of<'a>(
        written: &[(ImmutableString, ImmutableString)],
        regions: impl IntoIterator<Item = &'a Regions>,
    ) -> Self {
        let mut bounds: Vec<(Lifetime, Lifetime)> = written
            .iter()
            .map(|(longer, shorter)| (Lifetime::named(longer), Lifetime::named(shorter)))
            .collect();
        for regions in regions {
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
    pub(crate) receiver: Regions,
    pub(crate) parameters: Vec<Regions>,
    pub(crate) output: Output,
    /// Each elided input lifetime, by its number less one: the receiver's
    /// is at no place.
    pub(crate) elided: Vec<Elision>,
}

/// A function's output, as what its body returns is checked against it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Output {
    /// Its regions, the elided ones given by elision.
    regions: Regions,
    /// Its positions below which it is invariant, and what makes each so:
    /// what is returned there has its lifetimes exactly.
    invariant: Vec<(Path, Invariance)>,
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
    let mut receiver = Regions::new();
    let mut parameters = Vec::new();
    for (input, positions) in &read.inputs {
        let regions = positions
            .iter()
            .filter(generic)
            .map(|(path, region)| (*path, Lifetime::of(region)))
            .collect();
        match input {
            Input::Receiver => receiver = regions,
            Input::Argument(_) => parameters.push(regions),
        }
    }
    let bounds = Bounds::of(
        &signature.lifetimes.bounds,
        std::iter::once(&receiver)
            .chain(&parameters)
            .chain(std::iter::once(&output)),
    );
    Own {
        receiver,
        parameters,
        output: Output {
            regions: output,
            invariant: read.invariant,
            bounds,
        },
        elided: read.elided,
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
        let at = self.within(at);
        let mut below: Vec<(Path, &Lifetime)> = Vec::new();
        for (path, output) in &self.regions {
            match output {
                _ if !path.starts_with(at) => {}
                Some(output) => below.push((*path, output)),
                None => return Relation::Unsaid,
            }
        }
        let exact = matches!(below[..], [(path, _)] if path == at);
        if !exact && below.iter().any(|(_, output)| *output != below[0].1) {
            return Relation::Unsaid;
        }
        let invariance = |path: Path| {
            self.invariant
                .iter()
                .find(|(root, _)| exact && path.starts_with(*root))
                .map(|(_, invariance)| invariance)
        };
        below
            .into_iter()
            .find_map(|(path, output)| {
                let invariance = invariance(path);
                let direction = if invariance.is_some() && !self.bounds.outlives(output, lifetime) {
                    Direction::Output
                } else if !self.bounds.outlives(lifetime, output) {
                    Direction::Input
                } else {
                    return None;
                };
                Some(Required {
                    output,
                    direction,
                    invariance,
                })
            })
            .map_or(Relation::Holds, Relation::Broken)
    }

    /// `at`, or the own region above it that holds it where the output's
    /// type has no region at or below `at`: what a reborrow keeps below the
    /// reference it makes is within that reference's region.
    fn within(&self, at: Path) -> Path {
        let mut above = at;
        while !self.regions.iter().any(|(path, _)| path.starts_with(above)) {
            if above.is_root() {
                return at;
            }
            above = above.parent();
            if above.is_own() && self.regions.iter().any(|(path, _)| *path == above) {
                return above;
            }
        }
        at
    }
}

/// The positions of the crate type whose field or payload is of type `ty`,
/// below which it is invariant ([`Variance`]). Its lifetime parameters are
/// `regions`' and its type parameters `parameters`.
pub(crate) fn invariant_in(
    ty: &Type,
    regions: &crate::resolve::item::TypeRegions,
    parameters: &[ImmutableString],
    declared: &dyn Fn(&Target) -> Option<usize>,
    variance: Variance<'_>,
) -> Vec<Path> {
    let mut reader = Reader {
        lifetimes: &regions.written,
        declared,
        variance,
        elided: Vec::new(),
        invariant: Vec::new(),
    };
    let mut positions = Positions::new();
    reader.positions(ty, Path::ROOT, true, &mut positions);
    let mut invariant = Vec::new();
    for (path, region) in positions {
        if !reader
            .invariant
            .iter()
            .any(|(root, _)| path.starts_with(*root))
        {
            continue;
        }
        let step = match &region {
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
        if let Some(step) = step
            && !invariant.contains(&Path::of(&[step]))
        {
            invariant.push(Path::of(&[step]));
        }
    }
    invariant
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
