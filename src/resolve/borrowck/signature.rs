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
    elided: usize,
}

impl Reader<'_> {
    fn lifetime(&mut self, span: Option<Span>) -> Region {
        match span.and_then(|span| self.lifetimes.names.get(&span)) {
            Some(name) if name == "static" => Region::Static,
            Some(name) if name != "_" => Region::Named(name.clone()),
            _ => self.fresh(),
        }
    }

    fn fresh(&mut self) -> Region {
        self.elided += 1;
        Region::Elided(self.elided)
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
                lifetime, target, ..
            } => {
                let region = self.lifetime(*lifetime);
                out.push((own, region));
                self.positions(target, at.push(Step::Arg(0)), output, out);
            }
            // `&str`, which a resolved type reads as `String` with the
            // reference's lifetime.
            Type::Path(path) if ty.is_str() => {
                let span = match &*path.arguments {
                    [Argument::Lifetime(span)] => Some(*span),
                    _ => None,
                };
                let region = self.lifetime(span);
                out.push((own, region));
            }
            // An iterator's value captures what it was made of; as an
            // output, its items are below it.
            Type::Path(_) if ty.iterator_item().is_some() => {
                out.push((own, Region::Captures));
                if output && let Some((item, _)) = ty.iterator_item() {
                    self.positions(item, at.push(Step::Arg(0)), output, out);
                }
            }
            // A function value holds what it captures: within the
            // lifetimes its type is bounded by (`impl Fn() + 'a`), or any.
            Type::Path(path) if ty.fn_signature().is_some() => {
                let bounded: Vec<Region> = path
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Argument::Lifetime(span) => Some(self.lifetime(Some(*span))),
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
                    Target::Projection(..) | Target::Alias(_) | Target::Host { .. } => {
                        out.push((own, Region::Unknown));
                    }
                    Target::Builtin if opaque(&path.name) => out.push((own, Region::Captures)),
                    Target::Builtin | Target::Script { .. } => {}
                }
                let script = matches!(path.target, Target::Script { .. });
                let mut written = 0;
                let mut types = 0;
                for argument in &path.arguments {
                    match argument {
                        Argument::Type(ty) => {
                            self.positions(ty, at.push(Step::Arg(types)), output, out);
                            types += 1;
                        }
                        // A builtin's lifetime has no place in its value.
                        Argument::Lifetime(_) if !script => out.push((own, Region::Unknown)),
                        Argument::Lifetime(span) => {
                            let region = self.lifetime(Some(*span));
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
                                let fresh = self.fresh();
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

/// What a call of `signature` keeps of each of its inputs. `declared` says
/// how many lifetime parameters a crate type has.
pub(crate) fn summary(
    signature: &Signature<'_>,
    declared: &dyn Fn(&Target) -> Option<usize>,
) -> Summary {
    let mut reader = Reader {
        lifetimes: signature.lifetimes,
        declared,
        elided: 0,
    };
    let bounds = &signature.lifetimes.bounds;
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
                        _ => reader.fresh(),
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
    for (index, ty) in signature.parameters.iter().enumerate() {
        let mut positions = Positions::new();
        reader.positions(ty, Path::ROOT, false, &mut positions);
        inputs.push((Input::Argument(index), positions));
    }
    let mut output = Positions::new();
    reader.positions(signature.output, Path::ROOT, true, &mut output);

    // Elided output lifetimes are `&self`'s, or the one input lifetime.
    let elided_to = receiver_region.or_else(|| {
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
        match distinct[..] {
            [only] if !unknown => Some(only.clone()),
            _ => None,
        }
    });
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
        elided: 0,
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
