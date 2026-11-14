//! Which types satisfy a bound, read from std's impls (`impls.tsv`): the
//! types a generic parameter of a std item takes (`P: AsRef<Path>` takes a
//! `String`, a `PathBuf`, a `&str`, ..), whether a type implements a trait,
//! and the associated types an impl gives it.
//!
//! An impl's where-clauses are followed [`DEPTH`] impls deep: `&T:
//! AsRef<U>` holds through `T: AsRef<U>`, and no deeper, so `&&str` is
//! never one of a parameter's types.

use crate::std_table::{self, Impl, Written};

/// How many impls deep a bound is proved.
pub(super) const DEPTH: u8 = 2;

/// What an impl's parameters, or a declaration's names, stand for.
pub(super) type Binding = Vec<(&'static str, Written)>;

/// A type in a bound: known, or the one the bound is solved for.
#[derive(Clone, Debug)]
pub(super) enum Slot {
    Known(Written),
    Unknown,
}

/// The types the unknown slot of `self_ty: trait_<arguments>` stands for,
/// one for each way an impl proves it. With no unknown slot, a unit for
/// each impl that proves it.
pub(super) fn solve(trait_: &str, self_ty: &Slot, arguments: &[Slot], depth: u8) -> Vec<Written> {
    let mut found = Vec::new();
    let Some(depth) = depth.checked_sub(1) else {
        return found;
    };
    let known = match self_ty {
        Slot::Known(known) => Some(known),
        Slot::Unknown => None,
    };
    for implementation in impls(trait_, known) {
        let variables = variables(&implementation);
        let own = read(implementation.self_ty);
        let mut binding = Binding::new();
        let mut unknown = None;
        match self_ty {
            Slot::Known(known) => {
                if !matches(&own, known, &variables, &mut binding) {
                    continue;
                }
            }
            Slot::Unknown => unknown = Some(own.clone()),
        }
        // An impl's missing argument is its trait's default, `Self`, as
        // `PartialEq`'s `Rhs` is.
        let written = implementation.arguments();
        let fits = arguments.iter().enumerate().all(|(index, slot)| {
            let pattern = written
                .get(index)
                .map_or_else(|| own.clone(), |text| read(text));
            match slot {
                Slot::Known(known) => matches(&pattern, known, &variables, &mut binding),
                Slot::Unknown => {
                    unknown = Some(pattern);
                    true
                }
            }
        });
        if !fits {
            continue;
        }
        for binding in where_clauses(&implementation, &variables, binding, &[], depth) {
            found.push(match &unknown {
                Some(pattern) => substitute(pattern, &binding),
                None => Written::Tuple(Vec::new()),
            });
        }
    }
    found
}

/// The impls of `trait_` that may apply to `self_ty`, in the table's
/// order: those filed under its simplified key and the blanket ones. Any
/// other impl's self type is another type's, so none of them applies. With
/// no key known, every impl.
pub(super) fn impls(trait_: &str, self_ty: Option<&Written>) -> Vec<Impl> {
    let Some(key) = self_ty.and_then(key) else {
        return std_table::trait_impls(trait_).collect();
    };
    let own = std_table::impls(trait_, key);
    let blanket = std_table::impls(trait_, "_");
    // The table is sorted, keys too.
    if key < "_" {
        own.chain(blanket).collect()
    } else {
        blanket.chain(own).collect()
    }
}

/// The key `impls.tsv` files impls of `written` under, as rustc's
/// simplified type: a primitive's name, a type's definition, `&`, `&mut`,
/// `[]`, `[;]`, `()` or `!`. `None` for a name std doesn't define, as an
/// impl's parameter.
fn key(written: &Written) -> Option<&'static str> {
    match written {
        Written::Path { path, .. } => std_table::definition_of(path),
        Written::Reference { mutable: false, .. } => Some("&"),
        Written::Reference { mutable: true, .. } => Some("&mut"),
        Written::Slice(_) => Some("[]"),
        Written::Array(..) => Some("[;]"),
        Written::Tuple(_) => Some("()"),
        Written::Never => Some("!"),
        Written::Other(_) => None,
    }
}

/// Whether std implements `trait_<arguments>` for `self_ty`.
pub(super) fn holds(trait_: &str, self_ty: &Written, arguments: &[Written]) -> bool {
    let arguments = arguments
        .iter()
        .map(|argument| Slot::Known(argument.clone()))
        .collect::<Vec<_>>();
    !solve(trait_, &Slot::Known(self_ty.clone()), &arguments, DEPTH).is_empty()
}

/// Each way `implementation` applies to `self_ty`: what its parameters
/// stand for, with those its where-clauses solve. A clause on one of the
/// `held` parameters `self_ty` names holds: a held generic type's `T` is
/// whatever value rawr holds, which the call's own argument fixes.
pub(super) fn instances(
    implementation: &Impl,
    self_ty: &Written,
    held: &[&'static str],
) -> Vec<Binding> {
    let variables = variables(implementation);
    let mut binding = Binding::new();
    if !matches(
        &read(implementation.self_ty),
        self_ty,
        &variables,
        &mut binding,
    ) {
        return Vec::new();
    }
    where_clauses(implementation, &variables, binding, held, DEPTH - 1)
}

/// The associated type `name` of the first impl of `trait_` that applies
/// to `self_ty`: `String` of `<str as ToOwned>::Owned`.
pub(super) fn associated(trait_: &str, self_ty: &Written, name: &str) -> Option<Written> {
    impls(trait_, Some(self_ty))
        .into_iter()
        .find_map(|implementation| {
            let binding = instances(&implementation, self_ty, &[])
                .into_iter()
                .next()?;
            Some(substitute(
                &read(implementation.associated(name)?),
                &binding,
            ))
        })
}

/// The type `text` writes, without its lifetimes, which no bound reads:
/// `&'a mut T` is `&mut T`, `Cow<'_, str>` is `Cow<str>`.
pub(super) fn read(text: &'static str) -> Written {
    plain(std_table::written(text))
}

fn plain(written: Written) -> Written {
    match written {
        Written::Path {
            path,
            arguments,
            bindings,
        } => Written::Path {
            path,
            arguments: arguments
                .into_iter()
                .filter(|argument| {
                    !matches!(argument, Written::Path { path, .. } if path.starts_with('\''))
                })
                .map(plain)
                .collect(),
            bindings: bindings
                .into_iter()
                .map(|(name, bound)| (name, plain(bound)))
                .collect(),
        },
        // `&'a T` parses as a reference to `'a T`.
        Written::Reference {
            mutable: false,
            target,
        } if let Written::Other(text) = *target
            && text.starts_with('\'') =>
        {
            let rest = text.split_once(' ').map_or("", |(_, rest)| rest);
            match rest.strip_prefix("mut ") {
                Some(rest) => Written::Reference {
                    mutable: true,
                    target: Box::new(read(rest)),
                },
                None => Written::Reference {
                    mutable: false,
                    target: Box::new(read(rest)),
                },
            }
        }
        Written::Reference { mutable, target } => Written::Reference {
            mutable,
            target: Box::new(plain(*target)),
        },
        Written::Slice(element) => Written::Slice(Box::new(plain(*element))),
        Written::Array(element, length) => Written::Array(Box::new(plain(*element)), length),
        Written::Tuple(members) => Written::Tuple(members.into_iter().map(plain).collect()),
        other => other,
    }
}

/// `written` with each name `binding` binds replaced by what it stands for.
pub(super) fn substitute(written: &Written, binding: &[(&'static str, Written)]) -> Written {
    match written {
        Written::Path {
            path, arguments, ..
        } if arguments.is_empty()
            && let Some((_, bound)) = binding.iter().find(|(name, _)| name == path) =>
        {
            bound.clone()
        }
        Written::Path {
            path,
            arguments,
            bindings,
        } => Written::Path {
            path,
            arguments: arguments
                .iter()
                .map(|argument| substitute(argument, binding))
                .collect(),
            bindings: bindings
                .iter()
                .map(|(name, bound)| (*name, substitute(bound, binding)))
                .collect(),
        },
        Written::Reference { mutable, target } => Written::Reference {
            mutable: *mutable,
            target: Box::new(substitute(target, binding)),
        },
        Written::Slice(element) => Written::Slice(Box::new(substitute(element, binding))),
        Written::Array(element, length) => {
            Written::Array(Box::new(substitute(element, binding)), length)
        }
        Written::Tuple(members) => Written::Tuple(
            members
                .iter()
                .map(|member| substitute(member, binding))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether `a` and `b` are one type.
pub(super) fn same(a: &Written, b: &Written) -> bool {
    matches(a, b, &[], &mut Binding::new())
}

/// The definition a written path names: `core::io::error::Error` of
/// `std::io::Error`, or the path itself when it is a definition already.
pub(super) fn definition(path: &'static str) -> &'static str {
    std_table::definition_of(path).unwrap_or(path)
}

/// The impl's type parameters, which its header's types bind.
fn variables(implementation: &Impl) -> Vec<&'static str> {
    implementation
        .generics()
        .map(|name| name.strip_prefix("const:").unwrap_or(name))
        .collect()
}

/// Whether `concrete` is an instance of `pattern`, whose `variables` it
/// binds in `binding`.
fn matches(
    pattern: &Written,
    concrete: &Written,
    variables: &[&'static str],
    binding: &mut Binding,
) -> bool {
    match (pattern, concrete) {
        (
            Written::Path {
                path, arguments, ..
            },
            _,
        ) if arguments.is_empty() && variables.contains(path) => {
            match binding.iter().find(|(name, _)| name == path) {
                Some((_, bound)) => same(&bound.clone(), concrete),
                None => {
                    binding.push((path, concrete.clone()));
                    true
                }
            }
        }
        (
            Written::Path {
                path, arguments, ..
            },
            Written::Path {
                path: other,
                arguments: given,
                ..
            },
        ) => {
            (path == other || definition(path) == definition(other))
                && arguments
                    .iter()
                    .zip(given)
                    .all(|(pattern, concrete)| matches(pattern, concrete, variables, binding))
        }
        (
            Written::Reference { mutable, target },
            Written::Reference {
                mutable: other,
                target: given,
            },
        ) => mutable == other && matches(target, given, variables, binding),
        (Written::Slice(pattern), Written::Slice(concrete)) => {
            matches(pattern, concrete, variables, binding)
        }
        (Written::Array(pattern, length), Written::Array(concrete, given)) => {
            (length == given || variables.contains(length))
                && matches(pattern, concrete, variables, binding)
        }
        (Written::Tuple(patterns), Written::Tuple(members)) => {
            patterns.len() == members.len()
                && patterns
                    .iter()
                    .zip(members)
                    .all(|(pattern, concrete)| matches(pattern, concrete, variables, binding))
        }
        (Written::Other(pattern), Written::Other(concrete)) => pattern == concrete,
        (Written::Never, Written::Never) => true,
        _ => false,
    }
}

/// Each binding of `binding`'s extension under which the impl's
/// where-clauses hold: a clause on a parameter still free solves it.
fn where_clauses(
    implementation: &Impl,
    variables: &[&'static str],
    binding: Binding,
    held: &[&'static str],
    depth: u8,
) -> Vec<Binding> {
    let mut bindings = vec![binding];
    for (subject, bounds) in implementation.wheres() {
        for bound in std_table::split_bounds(bounds) {
            if bound == "?Sized" || bound.starts_with('\'') {
                continue;
            }
            let Written::Path {
                path, arguments, ..
            } = read(bound)
            else {
                return Vec::new();
            };
            let Some(trait_) = std_table::definition_of(path) else {
                return Vec::new();
            };
            bindings = bindings
                .into_iter()
                .flat_map(|binding| {
                    let subject = substitute(&read(subject), &binding);
                    if matches!(&subject, Written::Path { path, arguments, .. }
                        if arguments.is_empty() && held.contains(path))
                    {
                        return vec![binding];
                    }
                    let arguments = arguments
                        .iter()
                        .map(|argument| substitute(argument, &binding))
                        .collect::<Vec<_>>();
                    clause(trait_, &subject, &arguments, variables, binding, depth)
                })
                .collect();
        }
    }
    bindings
}

/// The extensions of `binding` under which `subject: trait_<arguments>`
/// holds: a free parameter that is the subject or an argument is solved
/// for, and any other free parameter fails the clause.
fn clause(
    trait_: &str,
    subject: &Written,
    arguments: &[Written],
    variables: &[&'static str],
    binding: Binding,
    depth: u8,
) -> Vec<Binding> {
    let free = |written: &Written| match written {
        Written::Path {
            path, arguments, ..
        } if arguments.is_empty() && variables.contains(path) => Some(*path),
        _ => None,
    };
    let mut slots = Vec::new();
    let mut solved = None;
    for written in std::iter::once(subject).chain(arguments) {
        match free(written) {
            Some(name) if solved.is_none() => {
                solved = Some(name);
                slots.push(Slot::Unknown);
            }
            _ if holds_free(written, variables) => return Vec::new(),
            _ => slots.push(Slot::Known(written.clone())),
        }
    }
    let self_slot = slots.remove(0);
    let found = solve(trait_, &self_slot, &slots, depth);
    match solved {
        Some(name) => found
            .into_iter()
            .map(|written| {
                let mut binding = binding.clone();
                binding.push((name, written));
                binding
            })
            .collect(),
        None if found.is_empty() => Vec::new(),
        None => vec![binding],
    }
}

/// Whether `written` names one of `variables`.
fn holds_free(written: &Written, variables: &[&'static str]) -> bool {
    match written {
        Written::Path {
            path,
            arguments,
            bindings,
        } => {
            (arguments.is_empty() && variables.contains(path))
                || arguments
                    .iter()
                    .any(|argument| holds_free(argument, variables))
                || bindings
                    .iter()
                    .any(|(_, bound)| holds_free(bound, variables))
        }
        Written::Reference { target, .. } => holds_free(target, variables),
        Written::Slice(element) | Written::Array(element, _) => holds_free(element, variables),
        Written::Tuple(members) => members.iter().any(|member| holds_free(member, variables)),
        _ => false,
    }
}
