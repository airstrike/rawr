//! std's trait impls on rawr's native types, from the std table (D711
//! step 3). An impl is selected as rustc selects one among std's: by the
//! trait and the self type's simplified key, then by matching its header's
//! types and proving its where-clauses the same way. A type std doesn't
//! declare (a program's, a crate's, a parameter) answers through the
//! program's own impls, derives and bounds.
//!
//! [`std_type`] and [`std_definition`] are the binding: which rawr type
//! stands for which std type. They state no signature, impl or arity.

use super::*;
use crate::std_table::{self, Written};

/// What an impl's parameters stand for once its header matched.
#[derive(Clone, Debug, Default)]
pub(super) struct Substitution {
    types: BTreeMap<&'static str, Ty>,
    lengths: BTreeMap<&'static str, usize>,
}

/// An impl the solver selected.
#[derive(Clone, Debug)]
pub(super) struct Selected {
    pub(super) line: std_table::Impl,
    substitution: Substitution,
}

/// Why no impl holds: the innermost self type and trait argument an impl
/// needed, as rustc's E0277 names them.
pub(super) type Unmet = (Ty, Option<Ty>);

/// How deep where-clauses are proven before giving up, as rustc's
/// recursion limit does.
const DEPTH: usize = 16;

/// The std definition the native type `ty` stands for, with its type
/// arguments: `alloc::string::String`, `alloc::vec::Vec` of a `Vec<T>`,
/// `i64`. `None` for a type std
/// doesn't declare, and for a literal whose kind is still to be inferred.
pub(super) fn std_definition(ty: &Ty) -> Option<(&'static str, Vec<Ty>)> {
    let one = |ty: &Ty| vec![ty.clone()];
    Some(match ty {
        Ty::Bool => ("bool", Vec::new()),
        Ty::Char => ("char", Vec::new()),
        Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_)) => return None,
        Ty::Int(kind) => (kind.name(), Vec::new()),
        Ty::Float(Float::F32) => ("f32", Vec::new()),
        Ty::Float(_) => ("f64", Vec::new()),
        Ty::String => ("alloc::string::String", Vec::new()),
        Ty::Array {
            element,
            kind: sequence::Kind::Vec,
        } => ("alloc::vec::Vec", one(element)),
        Ty::Array {
            element,
            kind: sequence::Kind::Deque,
        } => ("alloc::collections::vec_deque::VecDeque", one(element)),
        Ty::Option(inner) => ("core::option::Option", one(inner)),
        Ty::Result(ok, error) => (
            "core::result::Result",
            vec![(**ok).clone(), (**error).clone()],
        ),
        Ty::Boxed(inner) => ("alloc::boxed::Box", one(inner)),
        Ty::Counted(counter, inner) => (counter.std_paths().0, one(inner)),
        Ty::Weak(counter, inner) => (counter.std_paths().1, one(inner)),
        Ty::Keyed(kind, arguments) => (
            match kind {
                Keyed::HashMap => "std::collections::hash::map::HashMap",
                Keyed::HashSet => "std::collections::hash::set::HashSet",
                Keyed::BTreeMap => "alloc::collections::btree::map::BTreeMap",
                Keyed::BTreeSet => "alloc::collections::btree::set::BTreeSet",
            },
            arguments.to_vec(),
        ),
        Ty::Reverse(inner) => ("core::cmp::Reverse", one(inner)),
        Ty::Cow(borrowed) => (cow::DEFINITION, one(borrowed)),
        Ty::Cell(CellKind::RefCell, inner) => ("core::cell::RefCell", one(inner)),
        Ty::Cell(CellKind::Cell, inner) => ("core::cell::Cell", one(inner)),
        Ty::Ordering => ("core::cmp::Ordering", Vec::new()),
        Ty::Time(Time::Duration) => ("core::time::Duration", Vec::new()),
        Ty::Time(Time::Instant) => ("std::time::Instant", Vec::new()),
        Ty::ParseError(error) => (
            match error {
                ParseError::Int => "core::num::error::ParseIntError",
                ParseError::Float => "core::num::float_parse::ParseFloatError",
                ParseError::Bool => "core::str::error::ParseBoolError",
                ParseError::Char => "core::char::convert::ParseCharError",
                ParseError::Infallible => "core::convert::Infallible",
                ParseError::Utf8 => "alloc::string::FromUtf8Error",
                ParseError::TryFromInt => "core::num::error::TryFromIntError",
            },
            Vec::new(),
        ),
        Ty::Range(inner) => ("core::ops::range::Range", one(inner)),
        Ty::InclusiveRange => ("core::ops::range::RangeInclusive", vec![Ty::Int(Int::I64)]),
        Ty::FloatRange => (
            "core::ops::range::RangeInclusive",
            vec![Ty::Float(Float::F64)],
        ),
        _ => return None,
    })
}

/// The native type that stands for the std definition `definition` with
/// `arguments`: the inverse of [`std_definition`].
pub(super) fn std_type(definition: &str, arguments: &[Ty]) -> Option<Ty> {
    let first = || arguments.first().cloned().map(Box::new);
    if let Some(kind) = Int::named(definition) {
        return Some(Ty::Int(kind));
    }
    Some(match definition {
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "f32" => Ty::Float(Float::F32),
        "f64" => Ty::Float(Float::F64),
        "alloc::string::String" => Ty::String,
        "alloc::vec::Vec" => Ty::Array {
            element: first()?,
            kind: sequence::Kind::Vec,
        },
        "alloc::collections::vec_deque::VecDeque" => Ty::Array {
            element: first()?,
            kind: sequence::Kind::Deque,
        },
        "core::option::Option" => Ty::Option(first()?),
        "core::result::Result" => Ty::Result(first()?, Box::new(arguments.get(1)?.clone())),
        "alloc::boxed::Box" => Ty::Boxed(first()?),
        "alloc::rc::Rc" => Ty::Counted(Counter::Rc, first()?),
        "alloc::sync::Arc" => Ty::Counted(Counter::Arc, first()?),
        "alloc::rc::Weak" => Ty::Weak(Counter::Rc, first()?),
        "alloc::sync::Weak" => Ty::Weak(Counter::Arc, first()?),

        "std::collections::hash::map::HashMap" => {
            Ty::Keyed(Keyed::HashMap, arguments.get(..2)?.into())
        }
        "alloc::collections::btree::map::BTreeMap" => {
            Ty::Keyed(Keyed::BTreeMap, arguments.get(..2)?.into())
        }
        "std::collections::hash::set::HashSet" => {
            Ty::Keyed(Keyed::HashSet, arguments.get(..1)?.into())
        }
        "alloc::collections::btree::set::BTreeSet" => {
            Ty::Keyed(Keyed::BTreeSet, arguments.get(..1)?.into())
        }
        "core::cmp::Reverse" => Ty::Reverse(first()?),
        cow::DEFINITION => Ty::Cow(first()?),
        "core::cell::RefCell" => Ty::Cell(CellKind::RefCell, first()?),
        "core::cell::Cell" => Ty::Cell(CellKind::Cell, first()?),
        "core::cmp::Ordering" => Ty::Ordering,
        "core::time::Duration" => Ty::Time(Time::Duration),
        "std::time::Instant" => Ty::Time(Time::Instant),
        "core::num::error::ParseIntError" => Ty::ParseError(ParseError::Int),
        "core::num::float_parse::ParseFloatError" => Ty::ParseError(ParseError::Float),
        "core::str::error::ParseBoolError" => Ty::ParseError(ParseError::Bool),
        "core::char::convert::ParseCharError" => Ty::ParseError(ParseError::Char),
        "core::convert::Infallible" => Ty::ParseError(ParseError::Infallible),
        "alloc::string::FromUtf8Error" => Ty::ParseError(ParseError::Utf8),
        "core::num::error::TryFromIntError" => Ty::ParseError(ParseError::TryFromInt),
        "core::ops::range::Range" => Ty::Range(first()?),
        _ => return None,
    })
}

/// The last segment of a trait's definition: `PartialEq`.
fn trait_name(definition: &str) -> &str {
    definition.rsplit("::").next().unwrap_or(definition)
}

/// What the solver asks of the program being lowered: the parser itself,
/// which binds what is still to be inferred, or a [`Probe`] of it, which
/// only asks.
pub(super) trait Solve {
    fn resolved(&self, ty: &Ty) -> Ty;
    /// Whether two types are one, binding what is still to be inferred.
    fn unify(&mut self, expected: &Ty, actual: &Ty) -> bool;
    fn checkpoint(&mut self) -> Option<Checkpoint>;
    fn rollback(&mut self, checkpoint: Option<Checkpoint>);
    fn commit(&mut self, checkpoint: Option<Checkpoint>);
    /// A type to be inferred, for an impl parameter nothing bound.
    fn fresh(&mut self) -> Option<Ty>;
    /// Whether a type std doesn't declare implements the trait `name` with
    /// `arguments`: by its impl, derive or bound.
    fn program_implements(&mut self, ty: &Ty, name: &str, arguments: &[Ty]) -> bool;
}

impl Solve for Parser<'_> {
    fn resolved(&self, ty: &Ty) -> Ty {
        self.inference.resolve(ty)
    }

    fn unify(&mut self, expected: &Ty, actual: &Ty) -> bool {
        self.unify_type(expected, actual)
    }

    fn checkpoint(&mut self) -> Option<Checkpoint> {
        Some(self.inference.checkpoint())
    }

    fn rollback(&mut self, checkpoint: Option<Checkpoint>) {
        if let Some(checkpoint) = checkpoint {
            self.inference.rollback(checkpoint);
        }
    }

    fn commit(&mut self, checkpoint: Option<Checkpoint>) {
        if let Some(checkpoint) = checkpoint {
            self.inference.commit(checkpoint);
        }
    }

    fn fresh(&mut self) -> Option<Ty> {
        Some(self.inference.fresh())
    }

    fn program_implements(&mut self, ty: &Ty, name: &str, arguments: &[Ty]) -> bool {
        match capability::Kind::from_derive(name) {
            Some(kind) => self.type_provides(ty, kind),
            None => self.type_implements_with(ty, name, arguments),
        }
    }
}

/// A question to the solver from where nothing may be bound: a type still
/// to be inferred matches what it may become.
pub(super) struct Probe<'a, 'p>(pub(super) &'a Parser<'p>);

impl Solve for Probe<'_, '_> {
    fn resolved(&self, ty: &Ty) -> Ty {
        self.0.inference.resolve(ty)
    }

    fn unify(&mut self, expected: &Ty, actual: &Ty) -> bool {
        let (expected, actual) = (self.resolved(expected), self.resolved(actual));
        expected == actual
            || matches!(
                (&expected, &actual),
                (Ty::Infer(_), _)
                    | (_, Ty::Infer(_))
                    | (Ty::Int(Int::Var(_)), Ty::Int(_))
                    | (Ty::Int(_), Ty::Int(Int::Var(_)))
                    | (Ty::Float(Float::Var(_)), Ty::Float(_))
                    | (Ty::Float(_), Ty::Float(Float::Var(_)))
            )
    }

    fn checkpoint(&mut self) -> Option<Checkpoint> {
        None
    }

    fn rollback(&mut self, _: Option<Checkpoint>) {}

    fn commit(&mut self, _: Option<Checkpoint>) {}

    fn fresh(&mut self) -> Option<Ty> {
        None
    }

    fn program_implements(&mut self, ty: &Ty, name: &str, _: &[Ty]) -> bool {
        match capability::Kind::from_derive(name) {
            Some(kind) => self.0.type_provides(ty, kind),
            None => self.0.type_implements(ty, name),
        }
    }
}

/// The simplified keys `impls.tsv` files `side`'s impls under, as rustc's
/// simplified type: `None` for a type std doesn't declare.
fn keys(solver: &impl Solve, side: &Ty) -> Option<Vec<&'static str>> {
    Some(match solver.resolved(side) {
        Ty::UnsizedStr => vec!["str"],
        Ty::Str | Ty::Reference { mutable: false, .. } => vec!["&"],
        Ty::Reference { mutable: true, .. } => vec!["&mut"],
        Ty::Array { kind, .. } => vec![match kind {
            sequence::Kind::Array(_) => "[;]",
            sequence::Kind::Vec => "alloc::vec::Vec",
            sequence::Kind::Slice => "[]",
            sequence::Kind::Deque => "alloc::collections::vec_deque::VecDeque",
        }],
        Ty::Unit | Ty::Tuple(_) => vec!["()"],
        Ty::Never => vec!["!"],
        ty => vec![std_definition(&ty)?.0],
    })
}

/// The impl of the std trait `trait_` (its definition) for `side` that
/// takes `arguments`, as rustc selects it: the first whose header matches
/// and whose where-clauses hold, its matching kept. `strict` proves a
/// program type's comparison by its impl or derive; else it takes its own
/// type, and the use checks it after.
pub(super) fn select(
    solver: &mut impl Solve,
    trait_: &str,
    side: &Ty,
    arguments: &[Ty],
    strict: bool,
    depth: usize,
) -> Result<Selected, Unmet> {
    let unmet = || (side.clone(), arguments.first().cloned());
    if depth > DEPTH {
        return Err(unmet());
    }
    // A type std doesn't declare has only std's blanket impls.
    let mut keys = keys(solver, side).unwrap_or_default();
    keys.push("_");
    let mut deepest = None;
    for key in keys {
        for line in std_table::impls(trait_, key) {
            let checkpoint = solver.checkpoint();
            match try_impl(solver, line, side, arguments, strict, depth) {
                Ok(substitution) => {
                    solver.commit(checkpoint);
                    return Ok(Selected { line, substitution });
                }
                Err(inner) => {
                    solver.rollback(checkpoint);
                    if deepest.is_none() {
                        deepest = inner;
                    }
                }
            }
        }
    }
    Err(deepest.unwrap_or_else(unmet))
}

/// How many of std's impls of `trait_` have a self type `side` matches.
fn candidates(solver: &mut impl Solve, trait_: &str, side: &Ty) -> usize {
    let mut keys = keys(solver, side).unwrap_or_default();
    keys.push("_");
    let mut count = 0;
    for key in keys {
        for line in std_table::impls(trait_, key) {
            let checkpoint = solver.checkpoint();
            let parameters = line.generics().collect::<Vec<_>>();
            if bind(
                solver,
                &std_table::written(line.self_ty),
                side,
                &parameters,
                &mut Substitution::default(),
            ) {
                count += 1;
            }
            solver.rollback(checkpoint);
        }
    }
    count
}

/// `line`'s header matched against `side` and `arguments`, and its
/// where-clauses proven: its substitution, else the innermost unmet clause
/// (`None` when the header didn't match).
fn try_impl(
    solver: &mut impl Solve,
    line: std_table::Impl,
    side: &Ty,
    arguments: &[Ty],
    strict: bool,
    depth: usize,
) -> Result<Substitution, Option<Unmet>> {
    let parameters = line.generics().collect::<Vec<_>>();
    let mut substitution = Substitution::default();
    let written = line.arguments();
    if written.len() < arguments.len()
        || !bind(
            solver,
            &std_table::written(line.self_ty),
            side,
            &parameters,
            &mut substitution,
        )
    {
        return Err(None);
    }
    for (pattern, argument) in written.iter().zip(arguments) {
        if !bind(
            solver,
            &std_table::written(pattern),
            argument,
            &parameters,
            &mut substitution,
        ) {
            return Err(None);
        }
    }
    for (subject, bounds) in line.wheres() {
        let Some(subject) = instantiate(
            solver,
            &std_table::written(subject),
            &parameters,
            &substitution,
        ) else {
            // A clause on a type rawr has none of (`[T]: ToOwned`).
            continue;
        };
        for bound in std_table::split_bounds(bounds) {
            let Written::Path {
                path, arguments, ..
            } = std_table::written(bound)
            else {
                continue;
            };
            let Some(definition) = std_table::definition_of(path) else {
                continue;
            };
            let Some(arguments) = arguments
                .iter()
                .map(|argument| instantiate(solver, argument, &parameters, &substitution))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            holds(solver, definition, &subject, &arguments, strict, depth + 1).map_err(Some)?;
        }
    }
    Ok(substitution)
}

/// Whether `side` implements the std trait `trait_` with `arguments`: by
/// std's impls for a type std declares, else by the program's.
pub(super) fn holds(
    solver: &mut impl Solve,
    trait_: &str,
    side: &Ty,
    arguments: &[Ty],
    strict: bool,
    depth: usize,
) -> Result<(), Unmet> {
    let filled;
    let arguments = match with_defaults(solver, trait_, side, arguments) {
        Some(arguments) => {
            filled = arguments;
            &filled[..]
        }
        None => arguments,
    };
    if keys(solver, side).is_some() {
        return select(solver, trait_, side, arguments, strict, depth).map(|_| ());
    }
    let unmet = || (side.clone(), arguments.first().cloned());
    let ty = solver.resolved(side);
    let name = trait_name(trait_);
    // A literal of a kind still to be inferred: the kind rawr runs it as
    // stands for every kind it may take, and binds nothing.
    if strict && let Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_)) = ty {
        let standing = match ty {
            Ty::Int(_) => Ty::Int(Int::I64),
            _ => Ty::Float(Float::F64),
        };
        let arguments = arguments
            .iter()
            .map(|argument| {
                if solver.resolved(argument) == ty {
                    standing.clone()
                } else {
                    argument.clone()
                }
            })
            .collect::<Vec<_>>();
        let checkpoint = solver.checkpoint();
        let selected = select(solver, trait_, &standing, &arguments, strict, depth);
        solver.rollback(checkpoint);
        return selected.map(|_| ()).map_err(|_| unmet());
    }
    let types = arguments
        .iter()
        .map(|argument| (*argument != Ty::UnsizedStr).then(|| argument.clone()))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(unmet)?;
    // A comparison of a type std doesn't declare takes its own type: a
    // derive's, a builtin's, a parameter's bound by `T: PartialEq`.
    if matches!(name, "PartialEq" | "PartialOrd")
        && let [argument] = &types[..]
        && !solver.unify(&ty, argument)
    {
        return Err(unmet());
    }
    if !strict || solver.program_implements(&ty, name, &types) {
        Ok(())
    } else {
        Err(unmet())
    }
}

/// `arguments` of the std trait `trait_` with the defaults of those it
/// leaves out: `PartialEq` is `PartialEq<Self>`. `None` when none is left
/// out.
fn with_defaults(
    solver: &mut impl Solve,
    trait_: &str,
    side: &Ty,
    arguments: &[Ty],
) -> Option<Vec<Ty>> {
    let parameters = std_table::item(trait_)?
        .generics()
        .filter(|parameter| {
            parameter.kind == std_table::ParameterKind::Type
                && parameter.stability == std_table::Stability::Stable
        })
        .collect::<Vec<_>>();
    if parameters.len() <= arguments.len() {
        return None;
    }
    let mut filled = arguments.to_vec();
    for parameter in &parameters[arguments.len()..] {
        filled.push(match parameter.default? {
            "Self" => side.clone(),
            default => instantiate(
                solver,
                &std_table::written(default),
                &[],
                &Substitution::default(),
            )?,
        });
    }
    Some(filled)
}

/// Match the std type `pattern` against `side`, binding `parameters` in
/// `substitution`; a type still to be inferred is bound as it unifies.
fn bind(
    solver: &mut impl Solve,
    pattern: &Written,
    side: &Ty,
    parameters: &[&'static str],
    substitution: &mut Substitution,
) -> bool {
    // `str` binds only a parameter or `str`.
    let ty = Some(solver.resolved(side)).filter(|ty| *ty != Ty::UnsizedStr);
    match pattern {
        Written::Path {
            path, arguments, ..
        } if arguments.is_empty() && parameters.contains(path) => {
            match substitution.types.get(path).cloned() {
                None => {
                    let bound = match ty {
                        Some(ty) => ty,
                        None => Ty::UnsizedStr,
                    };
                    substitution.types.insert(path, bound);
                    true
                }
                Some(Ty::UnsizedStr) => ty.is_none(),
                Some(bound) => ty.is_some_and(|ty| solver.unify(&bound, &ty)),
            }
        }
        Written::Reference { mutable, target } => match ty {
            Some(Ty::Str) if !mutable => {
                bind(solver, target, &Ty::UnsizedStr, parameters, substitution)
            }
            Some(Ty::Reference {
                mutable: given,
                target: referent,
            }) if given == *mutable => bind(solver, target, &referent, parameters, substitution),
            _ => false,
        },
        Written::Slice(element) => match ty {
            Some(Ty::Array {
                element: given,
                kind: sequence::Kind::Slice,
            }) => bind(solver, element, &given, parameters, substitution),
            _ => false,
        },
        Written::Array(element, length) => match ty {
            Some(Ty::Array {
                element: given,
                kind: sequence::Kind::Array(given_length),
            }) => {
                let fits = match substitution.lengths.get(length) {
                    Some(bound) => *bound == given_length,
                    None if parameters
                        .iter()
                        .any(|parameter| parameter.strip_prefix("const:") == Some(*length)) =>
                    {
                        substitution.lengths.insert(length, given_length);
                        true
                    }
                    None => length.parse::<usize>().ok() == Some(given_length),
                };
                fits && bind(solver, element, &given, parameters, substitution)
            }
            _ => false,
        },
        Written::Tuple(members) => match ty {
            Some(Ty::Unit) => members.is_empty(),
            Some(Ty::Tuple(given)) if given.len() == members.len() => members
                .iter()
                .zip(given.iter())
                .all(|(member, given)| bind(solver, member, given, parameters, substitution)),
            _ => false,
        },
        Written::Never => matches!(ty, Some(Ty::Never)),
        Written::Path {
            path, arguments, ..
        } => {
            let Some(definition) = std_table::definition_of(path) else {
                return false;
            };
            if definition == "str" {
                // A type still to be inferred becomes `str`.
                return match ty {
                    None => true,
                    Some(ty @ Ty::Infer(_)) => solver.unify(&Ty::UnsizedStr, &ty),
                    Some(_) => false,
                };
            }
            let Some(ty) = ty else {
                return false;
            };
            // A literal still to be inferred takes a primitive's kind.
            if let Ty::Int(Int::Var(_)) | Ty::Float(Float::Var(_)) | Ty::Infer(_) = ty {
                return match std_type(definition, &[]) {
                    Some(primitive) if arguments.is_empty() => solver.unify(&primitive, &ty),
                    _ => false,
                };
            }
            let Some((given, given_arguments)) = std_definition(&ty) else {
                return false;
            };
            // Rawr's `Vec<T>` binds a declaration's `Vec<T, A>`: the
            // allocator is the default's, which binds nothing.
            given == definition
                && arguments
                    .iter()
                    .zip(given_arguments)
                    .all(|(argument, given)| {
                        bind(solver, argument, &given, parameters, substitution)
                    })
        }
        Written::Other(_) => false,
    }
}

/// The std type `pattern` with `substitution`'s types for its
/// `parameters`: `None` when it names a type rawr has none of. A parameter
/// nothing bound is a fresh type, to be inferred.
fn instantiate<S: Solve>(
    solver: &mut S,
    pattern: &Written,
    parameters: &[&'static str],
    substitution: &Substitution,
) -> Option<Ty> {
    let ty = |solver: &mut S, pattern| instantiate_ty(solver, pattern, parameters, substitution);
    Some(match pattern {
        Written::Path {
            path, arguments, ..
        } if arguments.is_empty() && parameters.contains(path) => {
            match substitution.types.get(path) {
                Some(bound) => bound.clone(),
                None => solver.fresh()?,
            }
        }
        Written::Reference { mutable, target } => {
            match instantiate(solver, target, parameters, substitution)? {
                Ty::UnsizedStr if !mutable => Ty::Str,
                Ty::UnsizedStr => return None,
                target => Ty::Reference {
                    mutable: *mutable,
                    target: Box::new(target),
                },
            }
        }
        Written::Slice(element) => Ty::Array {
            element: Box::new(ty(solver, element)?),
            kind: sequence::Kind::Slice,
        },
        Written::Array(element, length) => {
            let length = match substitution.lengths.get(length) {
                Some(length) => *length,
                None => length.parse().ok()?,
            };
            Ty::Array {
                element: Box::new(ty(solver, element)?),
                kind: sequence::Kind::Array(length),
            }
        }
        Written::Tuple(members) if members.is_empty() => Ty::Unit,
        Written::Tuple(members) => Ty::Tuple(
            members
                .iter()
                .map(|member| ty(solver, member))
                .collect::<Option<_>>()?,
        ),
        Written::Never => Ty::Never,
        Written::Path {
            path, arguments, ..
        } => {
            let definition = std_table::definition_of(path)?;
            if definition == "str" {
                return Some(Ty::UnsizedStr);
            }
            let arguments = arguments
                .iter()
                .map(|argument| ty(solver, argument))
                .collect::<Option<Vec<_>>>()?;
            std_type(definition, &arguments)?
        }
        Written::Other(_) => return None,
    })
}

fn instantiate_ty(
    solver: &mut impl Solve,
    pattern: &Written,
    parameters: &[&'static str],
    substitution: &Substitution,
) -> Option<Ty> {
    instantiate(solver, pattern, parameters, substitution)
}

/// The associated type `name` of a selected impl: `str` for `Target` of
/// `impl Deref for String`.
pub(super) fn associated(solver: &mut impl Solve, selected: &Selected, name: &str) -> Option<Ty> {
    let parameters = selected.line.generics().collect::<Vec<_>>();
    let written = std_table::written(selected.line.associated(name)?);
    instantiate(solver, &written, &parameters, &selected.substitution)
}

impl Parser<'_> {
    /// Whether `side` is a type std declares, whose impls are std's.
    pub(super) fn std_declared(&self, side: &Ty) -> bool {
        keys(&Probe(self), side).is_some()
    }

    /// The `PartialEq` impl `lhs == rhs` selects from std's (D711 step 3):
    /// `Own` when it takes the left operand's type, `Between` when it
    /// compares two types, or the innermost two types an impl needed.
    pub(super) fn std_partial_eq(&mut self, lhs: &Ty, rhs: &Ty) -> Result<PartialEqImpl, (Ty, Ty)> {
        let resolve = |this: &Self, side: &Ty| this.inference.resolve(side);
        let (lhs, rhs) = (resolve(self, lhs), resolve(self, rhs));
        // A right operand still to be inferred takes the left's type.
        if matches!(rhs, Ty::Infer(_) | Ty::Unknown | Ty::Never) {
            return Ok(PartialEqImpl::Own);
        }
        // A scalar compares as a builtin operator, which takes its own type
        // (rustc's E0308 otherwise), as does a type std doesn't declare,
        // whose comparison the use checks.
        let scalar = |side: &Ty| matches!(side, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char);
        if scalar(&lhs) || keys(self, &lhs).is_none() {
            return Ok(PartialEqImpl::Own);
        }
        // One impl for the left operand's type fixes the right's, as
        // rustc's inference does before it checks the right operand
        // (`Option<T>`'s, a tuple's): it takes its own type.
        if candidates(self, "core::cmp::PartialEq", &lhs) == 1 {
            return Ok(PartialEqImpl::Own);
        }
        match select(
            self,
            "core::cmp::PartialEq",
            &lhs,
            std::slice::from_ref(&rhs),
            false,
            0,
        ) {
            // An impl that compared two values of one type is its own.
            Ok(_) if resolve(self, &lhs) == resolve(self, &rhs) => Ok(PartialEqImpl::Own),
            Ok(_) => Ok(PartialEqImpl::Between),
            Err((inner, argument)) => Err((inner, argument.unwrap_or(rhs))),
        }
    }

    /// Whether a value of `ty` has std's `PartialOrd` (`partial`) or `Ord`
    /// with its own type, from std's impls (D711 step 3).
    pub(super) fn std_ordered(&self, ty: &Ty, partial: bool) -> bool {
        let trait_ = if partial {
            "core::cmp::PartialOrd"
        } else {
            "core::cmp::Ord"
        };
        let side = self.inference.resolve(ty);
        let arguments = if partial {
            vec![side.clone()]
        } else {
            Vec::new()
        };
        holds(&mut Probe(self), trait_, &side, &arguments, true, 0).is_ok()
    }
}

impl Parser<'_> {
    /// Whether std has `impl From<source> for target` (D711 step 3), its
    /// where-clauses proven; nothing is bound.
    pub(super) fn std_from(&mut self, target: &Ty, source: &Ty) -> bool {
        let checkpoint = self.inference.checkpoint();
        let target = self.inference.resolve(target);
        let source = self.inference.resolve(source);
        let holds = select(self, "core::convert::From", &target, &[source], true, 0).is_ok();
        self.inference.rollback(checkpoint);
        holds
    }
}

/// How rawr runs std's `impl From<source> for target`, both resolved
/// (D711's execution mapping): the value as it is, where the two types
/// have one runtime form, or a prelude op. `None` when rawr doesn't run it
/// yet. Which impls exist is the table's to say.
pub(super) fn runs_from(target: &Ty, source: &Ty) -> Option<Option<PreludeOp>> {
    let text = |ty: &Ty| matches!(ty, Ty::String | Ty::Str);
    // One runtime form: a literal of a kind still to be inferred takes the
    // kind the impl names.
    let same = |a: &Ty, b: &Ty| {
        a == b
            || matches!(
                (a, b),
                (Ty::Int(_), Ty::Int(_)) | (Ty::Float(_), Ty::Float(_))
            )
    };
    match (target, source) {
        _ if target == source => Some(None),
        (Ty::String, Ty::Str) => Some(None),
        (Ty::String, Ty::Reference { target, .. }) if text(target) => Some(None),
        // Every integer is an `i64` at runtime, and std's `From`s among
        // them only widen.
        (Ty::Int(_), Ty::Int(_)) => Some(None),
        // An `f32` is an `f64` rounded at runtime.
        (Ty::Float(Float::F64), Ty::Float(_)) => Some(None),
        // Every sequence is an array at runtime.
        (Ty::Array { .. }, Ty::Array { .. }) => Some(None),
        // A `Cow` of a reference borrows it, and of a value owns it.
        (Ty::Cow(_), Ty::Str | Ty::Reference { .. }) => Some(Some(PreludeOp::CowBorrowed)),
        (Ty::Cow(_), _) => Some(Some(PreludeOp::CowOwned)),
        // A box is its value at runtime, and an `Rc` or `Arc` a new
        // allocation of it.
        (Ty::Boxed(inner), source) if same(inner, source) => Some(None),
        (Ty::Counted(_, inner), source) if same(inner, source) => Some(Some(PreludeOp::CountedNew)),
        _ => {
            let (to, from) = (builtin_code(target)?, builtin_code(source)?);
            BUILTIN_CONVERSIONS
                .iter()
                .find(|(target, source, _)| *target == to && *source == from)
                .map(|(.., op)| Some(*op))
        }
    }
}

impl Parser<'_> {
    /// How rawr runs `target::from(source)` when std has that impl
    /// (`runs_from`).
    pub(super) fn std_conversion(&mut self, target: &Ty, source: &Ty) -> Option<Option<PreludeOp>> {
        let (target, source) = (
            self.inference.resolve(target),
            self.inference.resolve(source),
        );
        if !self.std_from(&target, &source) {
            return None;
        }
        runs_from(&target, &source)
    }

    /// rawr's not-yet error (D712.1) for a conversion std has and rawr
    /// doesn't run: `From<u8> for f64`.
    pub(super) fn conversion_not_yet(
        &mut self,
        target: &Ty,
        source: &Ty,
        reference: Span,
    ) -> Option<Error> {
        let (target, source) = (
            self.inference.resolve(target),
            self.inference.resolve(source),
        );
        (self.std_from(&target, &source) && runs_from(&target, &source).is_none()).then(|| {
            Error::NotYet {
                path: format!(
                    "impl From<{}> for {}",
                    source.name(&self.models),
                    target.name(&self.models)
                )
                .into(),
                reference,
            }
        })
    }
}

/// One step of a deref, and how rawr runs it (D711's execution mapping
/// for std's `Deref` impls).
#[derive(Clone)]
pub(super) enum DerefCall {
    /// The target is the value itself at runtime: std's impls on rawr's
    /// natives, as a `String`'s `str`, a box's value, a reference's.
    Value,
    /// The program's `deref` (or `deref_mut`), called.
    Program(Box<method::Definition>),
    /// An `Rc`'s or `Arc`'s value, read (`counted.rs`).
    Counted,
    /// A crate type's `deref` (or `deref_mut`), called through its facade.
    #[cfg(feature = "rust-import")]
    Host { mutable: bool },
    /// A `Cow`'s payload, read (`cow.rs`).
    Cow,
}

impl Parser<'_> {
    /// `<ty as Deref>::Target` (D711 step 3, one rule for coercion, the
    /// method probe and `as_deref`): from std's impls on natives, the
    /// program's `impl Deref`, as `str` for a `String`. With `mutable`,
    /// through `DerefMut`. `None` when `ty` has no `Deref`.
    pub(super) fn deref_target(&mut self, ty: &Ty, mutable: bool) -> Option<(Ty, DerefCall)> {
        let ty = self.inference.resolve(ty);
        if let Ty::Script(id, _) = &ty {
            let (trait_name, function) = if mutable {
                ("DerefMut", "deref_mut")
            } else {
                ("Deref", "deref")
            };
            // std's trait, which the program implements: not one of its
            // own by that name.
            let std = self
                .models
                .traits
                .get(trait_name)
                .is_some_and(|(_, declared)| declared.std.is_some());
            if !std {
                return None;
            }
            let call = self
                .models
                .trait_methods
                .get(&(*id, ImmutableString::from(function)))?
                .iter()
                .find(|definition| {
                    definition
                        .trait_name()
                        .is_some_and(|name| name == trait_name)
                })?
                .clone();
            let target = self.inference.resolve(&Ty::Projection(
                Box::new(ty.clone()),
                "Deref".into(),
                "Target".into(),
            ));
            return Some((target, DerefCall::Program(Box::new(call))));
        }
        #[cfg(feature = "rust-import")]
        if matches!(ty, Ty::Host { .. })
            && let Some(target) = self.host_deref_target(&ty, mutable)
        {
            return Some((target, DerefCall::Host { mutable }));
        }
        let trait_ = if mutable {
            "core::ops::deref::DerefMut"
        } else {
            "core::ops::deref::Deref"
        };
        let checkpoint = self.inference.checkpoint();
        let side = ty.clone();
        let target = select(self, trait_, &side, &[], false, 0)
            .ok()
            .and_then(|_| {
                // `DerefMut`'s target is its `Deref`'s.
                let selected =
                    select(self, "core::ops::deref::Deref", &side, &[], false, 0).ok()?;
                associated(self, &selected, "Target")
            });
        self.inference.commit(checkpoint);
        let call = match ty {
            Ty::Counted(..) => DerefCall::Counted,
            Ty::Cow(_) => DerefCall::Cow,
            _ => DerefCall::Value,
        };
        target.map(|target| (target, call))
    }

    /// One deref step of the reference `reference`: `&Target` of `&T`,
    /// shared when `shared` (rustc's reborrow of a `&mut T` before it
    /// derefs), with the call it makes.
    pub(super) fn deref_reference(
        &mut self,
        reference: &Ty,
        shared: bool,
    ) -> Option<(Ty, DerefCall)> {
        let Ty::Reference { mutable, target } = self.inference.resolve(reference) else {
            return None;
        };
        let mutable = mutable && !shared;
        let (target, call) = self.deref_target(&target, mutable)?;
        let ty = match target {
            Ty::UnsizedStr if !mutable => Ty::Str,
            // rawr has no `&mut str`.
            Ty::UnsizedStr => return None,
            target => Ty::Reference {
                mutable,
                target: Box::new(target),
            },
        };
        Some((ty, call))
    }

    /// Deref coercion (D711 step 3): a reference where `expected`, another
    /// reference, is taken, through as many deref steps as make it one:
    /// `&String` to `&str`, `&&T` to `&T`, `&Box<T>` to `&T`, a program's
    /// `&Wrapper` to `&Target`. The type it ends at and each step's call;
    /// `None` when it is `expected` already or no steps make it one.
    pub(super) fn deref_coercion(
        &mut self,
        expected: &Ty,
        source: &Ty,
    ) -> Option<(Ty, Vec<DerefCall>)> {
        let shared = match self.inference.resolve(expected) {
            Ty::Str => true,
            Ty::Reference { mutable, target } => {
                // An unsizing coercion's, or one inference still decides.
                if matches!(
                    self.inference.resolve(&target),
                    Ty::Infer(_) | Ty::Dyn(..) | Ty::Unknown
                ) {
                    return None;
                }
                !mutable
            }
            _ => return None,
        };
        let mut current = self.inference.resolve(source);
        // A `&mut T` where `&T` is taken is the reborrow's.
        let reborrowed = match &current {
            Ty::Reference {
                mutable: true,
                target,
            } if shared => Ty::Reference {
                mutable: false,
                target: target.clone(),
            },
            _ => current.clone(),
        };
        if !matches!(current, Ty::Reference { .. })
            || self.unifies(expected, &current)
            || self.unifies(expected, &reborrowed)
        {
            return None;
        }
        let mut calls = Vec::new();
        for _ in 0..DEPTH {
            let (next, call) = self.deref_reference(&current, shared)?;
            // A `Vec<T>`'s `[T]` is rawr's type of it already.
            if next == current {
                return None;
            }
            calls.push(call);
            current = next;
            if self.unifies(expected, &current) {
                return Some((current, calls));
            }
        }
        None
    }

    /// `value` through `calls`, the deref steps a coercion or a method probe
    /// takes, as type `ty`: each program `deref` called on the last, and
    /// each crate type's through its facade.
    pub(super) fn apply_derefs(
        &mut self,
        value: &mut ParsedExpr,
        calls: Vec<DerefCall>,
        ty: Ty,
    ) -> Result<(), Error> {
        for call in calls {
            match call {
                DerefCall::Value => {}
                DerefCall::Program(call) => {
                    value.source = Self::std_call(&call, value.rendered_source(), &[]);
                    value.prelude = origin::Fragment::default();
                    value.lvalue = None;
                }
                DerefCall::Counted => self.counted_value(value),
                #[cfg(feature = "rust-import")]
                DerefCall::Host { mutable } => {
                    let name = if mutable { "deref_mut" } else { "deref" };
                    let at = value.span;
                    self.pending_arguments = Some((Vec::new(), at.end()));
                    let receiver = value.clone();
                    *value = self.parse_host_method(receiver, name.into(), at)?;
                }
                DerefCall::Cow => {
                    value.source = self.prelude_op(PreludeOp::CowValue, [value.rendered_source()]);
                    value.prelude = origin::Fragment::default();
                    value.lvalue = None;
                }
            }
        }
        value.ty = ty;
        Ok(())
    }

    /// The method probe's deref steps, as rustc's probe takes them: while
    /// `value`'s type, or the type it refers to, is a program's or a
    /// crate's type that lacks `member`, one step through its `Deref`. A
    /// method found past the steps that takes `&mut self` takes them
    /// through `DerefMut`, each `deref_mut` called on the place before it,
    /// as rustc's `convert_place_derefs_to_mutable` does.
    pub(super) fn probe_derefs(
        &mut self,
        mut value: ParsedExpr,
        member: &ImmutableString,
        member_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let mut steps = Vec::new();
        let mut probed = self.inference.resolve(&value.ty);
        loop {
            let target = match &probed {
                Ty::Reference { target, .. } => self.inference.resolve(target),
                owned => owned.clone(),
            };
            let own = match &target {
                Ty::Script(id, _) => self.own_member(*id, member),
                #[cfg(feature = "rust-import")]
                Ty::Host { .. } => self.host_own_member(&target, member),
                _ => break,
            };
            if own {
                break;
            }
            let reference = match &probed {
                reference @ Ty::Reference { .. } => reference.clone(),
                owned => Ty::Reference {
                    mutable: false,
                    target: Box::new(owned.clone()),
                },
            };
            let Some((ty, call)) = self.deref_reference(&reference, true) else {
                break;
            };
            steps.push((target, ty.clone(), call));
            probed = ty;
        }
        let method = matches!(self.peek(), Token::LeftParen | Token::Unit);
        if !method || self.member_receiver(&probed, member) != Some(receiver::Mode::Mutable) {
            for (_, ty, call) in steps {
                self.apply_derefs(&mut value, vec![call], ty)?;
            }
            return Ok(value);
        }
        for (owner, _, _) in steps {
            // rustc's E0596 when the type has no `DerefMut`.
            let Some((target, call)) = self.deref_target(&owner, true) else {
                return Err(Error::BorrowConflict {
                    code: "E0596",
                    message: format!(
                        "cannot borrow data in dereference of `{}` as mutable",
                        owner.name(&self.models)
                    )
                    .into(),
                    access: value.span,
                    borrow: Span::NONE,
                });
            };
            let Ty::Script(id, _) = owner else {
                let ty = match target {
                    Ty::UnsizedStr => Ty::Str,
                    target => Ty::Reference {
                        mutable: true,
                        target: Box::new(target),
                    },
                };
                self.apply_derefs(&mut value, vec![call], ty)?;
                continue;
            };
            let name = self.models.definition(id).name().into();
            value = match self.call_owner_method(
                value,
                id,
                name,
                &"deref_mut".into(),
                member_span,
                Vec::new(),
                member_span.start(),
            )? {
                Ok(called) => called,
                Err(_) => unreachable!("`DerefMut` has `deref_mut`"),
            };
        }
        Ok(value)
    }

    /// The receiver `member` takes on `ty`, through its references: a
    /// program type's method's, a crate type's, or std's inherent method's.
    fn member_receiver(&self, ty: &Ty, member: &str) -> Option<receiver::Mode> {
        let mut ty = self.inference.resolve(ty);
        while let Ty::Reference { target, .. } = ty {
            ty = self.inference.resolve(&target);
        }
        match ty {
            Ty::Script(id, _) => self.receiver_method(id, member)?.receiver(),
            #[cfg(feature = "rust-import")]
            ty @ Ty::Host { .. } => {
                let owner = self.host_member_owner(&ty, member)?;
                let items = self
                    .models
                    .host_items(&format!("{}::{member}", owner.path()))?;
                items
                    .iter()
                    .find_map(crate::host::item::Definition::receiver)
            }
            ty => std_method::inherent(&ty, member).map(|found| found.mode),
        }
    }

    /// `<ty as Deref>::Target` of a crate's type `ty`, or `DerefMut`'s with
    /// `mutable`: the target of the borrow guard its facade's `deref` (or
    /// `deref_mut`) gives (D554). `None` when the facade has none.
    #[cfg(feature = "rust-import")]
    fn host_deref_target(&mut self, ty: &Ty, mutable: bool) -> Option<Ty> {
        let (name, guard) = if mutable {
            ("deref_mut", crate::host::item::Guard::Mutable)
        } else {
            ("deref", crate::host::item::Guard::Shared)
        };
        let owner = self.host_member_owner(ty, name)?;
        let items = self
            .models
            .host_items(&format!("{}::{name}", owner.path()))?
            .to_vec();
        if !items
            .iter()
            .all(|item| item.guard() == Some(guard) && item.signature().parameters().is_empty())
        {
            return None;
        }
        let checkpoint = self.inference.checkpoint();
        let selected = self.select_host_signature(&items, Some(ty), &mut [], false, Span::NONE);
        let target = selected
            .ok()
            .map(|(_, signature)| self.inference.resolve(&signature.output));
        self.inference.commit(checkpoint);
        target
    }

    /// Whether the crate's type `ty` has a field or method `member` of its
    /// own, which the method probe finds before it derefs.
    #[cfg(feature = "rust-import")]
    fn host_own_member(&self, ty: &Ty, member: &str) -> bool {
        self.host_member_owner(ty, member).is_some()
            || ty.host_alias().is_some_and(|definition| {
                matches!(definition.shape(), crate::host::type_def::Shape::Struct { fields, .. }
                    if fields.iter().any(|field| field.name() == member))
            })
            || self.imported_field_owner(ty, member).is_some()
    }

    /// The definition of the crate's type `ty` whose facade declares the
    /// function `member`, at the path the type was named by or another.
    #[cfg(feature = "rust-import")]
    fn host_member_owner(
        &self,
        ty: &Ty,
        member: &str,
    ) -> Option<crate::host::type_def::Definition> {
        ty.host_alias()
            .filter(|definition| {
                self.models
                    .host_items(&format!("{}::{member}", definition.path()))
                    .is_some()
            })
            .cloned()
            .or_else(|| self.imported_member_owner(ty, member))
    }

    /// Whether the program's type `id` has a field or method `member` of its
    /// own, which the method probe finds before it derefs.
    pub(super) fn own_member(&self, id: type_def::Id, member: &str) -> bool {
        self.models.method(id, member).is_some()
            || self
                .models
                .trait_methods
                .contains_key(&(id, ImmutableString::from(member)))
            || matches!(self.models.definition(id), Nominal::Struct { item, .. }
                if item.fields.iter().any(|field| field.name.text == member))
    }
}
