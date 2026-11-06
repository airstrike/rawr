//! Opaque `impl Trait` types that a crate's functions give, as
//! `fn iced(..) -> impl Widget<Message, Theme, Renderer>` or
//! `fn application(..) -> Application<impl Program<..>>` do. Each is a type
//! of its own, named by its compiler definition: Rust code can't write it,
//! so a facade declares it under a name of its own and a runner reaches
//! its values only where the call that made one is compiled.

use std::collections::BTreeSet;

use crate::api::{self, model};

/// The identities of the opaque types the functions in `graph` give,
/// directly or as the associated type an implementation's method returns,
/// as `impl Fn(B) -> O` is of iced's `Function::with` for every `Fn`.
pub fn identities<P: api::Phase>(graph: &api::Graph<P>) -> BTreeSet<[u8; 32]> {
    let mut found = BTreeSet::new();
    for definition in graph.definitions() {
        match definition.kind() {
            model::Kind::Function { signature, .. } => collect(signature.output(), &mut found),
            model::Kind::AssociatedType {
                default: model::Default::Present(ty),
                ..
            } => collect(ty, &mut found),
            _ => {}
        }
    }
    found
}

fn collect(ty: &model::Type, found: &mut BTreeSet<[u8; 32]>) {
    match ty {
        model::Type::ImplTrait { id, .. } => {
            found.insert(*crate::surface::identity::Definition::of(id).as_bytes());
        }
        model::Type::Definition { arguments, .. } => {
            for argument in arguments.iter() {
                if let model::Argument::Type(ty) = argument {
                    collect(ty, found);
                }
            }
        }
        model::Type::Reference { ty, .. } | model::Type::Slice(ty) => collect(ty, found),
        model::Type::Array { element, .. } => collect(element, found),
        model::Type::Tuple(members) => {
            for member in members.iter() {
                collect(member, found);
            }
        }
        _ => {}
    }
}

/// Whether the facade path `path` names an opaque type a function gives
/// (`path`).
#[must_use]
pub fn declared(path: &str) -> bool {
    path.rsplit("::")
        .next()
        .is_some_and(|name| name.starts_with(PREFIX))
}

/// The start of the name of each opaque type a facade declares.
const PREFIX: &str = "__rawr_opaque_";

/// The name a facade declares the opaque type a function at `path` gives
/// under: beside the function, as `widget::__rawr_opaque_iced` for
/// `widget::iced`. Several opaque types one function gives are numbered
/// after the first.
#[must_use]
pub fn path(function: &str, index: usize) -> String {
    let (parent, name) = match function.rsplit_once("::") {
        Some((parent, name)) => (Some(parent), name),
        None => (None, function),
    };
    let name = if index == 0 {
        format!("{PREFIX}{name}")
    } else {
        format!("{PREFIX}{name}_{index}")
    };
    match parent {
        Some(parent) => format!("{parent}::{name}"),
        None => name,
    }
}

/// The start of the name of each function a facade declares beside a
/// trait's methods over its `Self` (`surface::member`) for a type
/// parameter of one of its methods, or an associated type of one, as
/// `serialize<S: Serializer>`'s `S` and `S::Ok` are (#593): a function no
/// program calls, which gives the opaque type the parameter is where native
/// code calls the program's impl of the method.
const PARAMETER: &str = "__rawr_param_";

/// Whether `path` is the function of [`parameter_name`] beside a trait's
/// methods over its `Self`.
#[must_use]
pub fn is_parameter(path: &str) -> bool {
    path.rsplit("::")
        .next()
        .is_some_and(|name| name.starts_with(PARAMETER))
}

/// The type parameter at `index` among the type parameters of the method
/// `method`, or its associated type `projection`, as one of a trait's
/// methods over its `Self` names it (`PARAMETER`).
#[must_use]
pub fn parameter_name(method: &str, index: usize, projection: Option<&str>) -> String {
    match projection {
        Some(associated) => format!("{PARAMETER}{method}_{index}_{associated}"),
        None => format!("{PARAMETER}{method}_{index}"),
    }
}

/// The path a facade declares the opaque type of the type parameter at
/// `index` of the method `method` of the trait at `trait_path` under, or
/// of its associated type `projection` (`parameter_name`).
#[must_use]
pub fn parameter_path(
    trait_path: &str,
    method: &str,
    index: usize,
    projection: Option<&str>,
) -> String {
    path(
        &super::member::path(trait_path, &parameter_name(method, index, projection)),
        0,
    )
}

/// The path of the opaque type `associated` of the type parameter whose
/// opaque type is at `path` (`parameter_path`), as `S::Ok` of `S`.
#[must_use]
pub fn parameter_projection(path: &str, associated: &str) -> Option<String> {
    let (method, index, None) = parameter_of(path)? else {
        return None;
    };
    let (owner, _) = path.rsplit_once("::")?;
    let name = parameter_name(method, index, Some(associated));
    Some(format!("{owner}::{PREFIX}{name}"))
}

/// The method, the index of the type parameter and the associated type a
/// facade path of [`parameter_path`] names.
#[must_use]
pub fn parameter_of(path: &str) -> Option<(&str, usize, Option<&str>)> {
    let name = path.rsplit("::").next()?;
    let rest = name.strip_prefix(PREFIX)?.strip_prefix(PARAMETER)?;
    // A method's name may hold `_`, so the index is the last segment of
    // digits that a projection's name or nothing follows.
    let mut parts = rest.rsplitn(3, '_');
    let last = parts.next()?;
    if let Ok(index) = last.parse::<usize>() {
        let method = rest.strip_suffix(last)?.strip_suffix('_')?;
        return Some((method, index, None));
    }
    let index = parts.next()?.parse::<usize>().ok()?;
    let method = parts.next()?;
    Some((method, index, Some(last)))
}

/// The names of an opaque type's parameters, `named` as the function's
/// parameters its arguments are, with each that is the value of an
/// associated type it binds named after that type, as `State` of `impl
/// Program<State = S>`, so no two share a name.
#[must_use]
pub fn parameters(
    named: Box<[Box<str>]>,
    associated: &[(
        crate::classify::representability::signature::Associated,
        u32,
    )],
) -> Box<[Box<str>]> {
    let mut names = named.into_vec();
    let future = *crate::surface::identity::Definition::future().as_bytes();
    for (associated, position) in associated {
        // The mark that the type is a future names the same argument as
        // its `Future::Output` does.
        if associated.trait_identity() == future {
            continue;
        }
        let Some(at) = usize::try_from(*position)
            .ok()
            .filter(|at| *at < names.len())
        else {
            continue;
        };
        let mut name = associated.name().to_owned();
        while names.iter().any(|known| **known == *name) {
            name.push('_');
        }
        names[at] = name.into_boxed_str();
    }
    names.into_boxed_slice()
}
