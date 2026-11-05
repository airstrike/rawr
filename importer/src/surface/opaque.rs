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

/// The type rustc gives an opaque type, `hidden` at the positions of its
/// parameters (`representability::signature::Opaque::hidden`), at the
/// opaque type's `arguments`: `Explain<Element<'_, ..>>` of iced's
/// `Widget::explain` on an element. `None` when it names a position past
/// them.
#[must_use]
pub fn hidden_at(
    hidden: &crate::classify::representability::convert::Type,
    arguments: &[crate::classify::representability::convert::Type],
) -> Option<crate::classify::representability::convert::Type> {
    use crate::classify::representability::convert::Type;
    let inside = |ty: &Type| hidden_at(ty, arguments).map(Box::new);
    let all = |types: &[Type]| {
        types
            .iter()
            .map(|ty| hidden_at(ty, arguments))
            .collect::<Option<Box<[_]>>>()
    };
    Some(match hidden {
        Type::Parameter(position) => arguments.get(usize::try_from(*position).ok()?)?.clone(),
        Type::Nominal {
            definition,
            arguments: inner,
        } => Type::Nominal {
            definition: *definition,
            arguments: all(inner)?,
        },
        Type::Script {
            path,
            arguments: inner,
        } => Type::Script {
            path: path.clone(),
            arguments: all(inner)?,
        },
        Type::Tuple(members) => Type::Tuple(all(members)?),
        Type::Option(inner) => Type::Option(inside(inner)?),
        Type::Range(inner) => Type::Range(inside(inner)?),
        Type::Result(ok, err) => Type::Result(inside(ok)?, inside(err)?),
        Type::Sequence {
            element,
            owned,
            length,
        } => Type::Sequence {
            element: inside(element)?,
            owned: *owned,
            length: *length,
        },
        Type::Borrowed { mutable, ty } => Type::Borrowed {
            mutable: *mutable,
            ty: inside(ty)?,
        },
        Type::FunctionItem { input, output } => Type::FunctionItem {
            input: inside(input)?,
            output: inside(output)?,
        },
        Type::ProfileMessage { mapper, payload } => Type::ProfileMessage {
            mapper: *mapper,
            payload: inside(payload)?,
        },
        Type::Unit | Type::Never | Type::Builtin(_) | Type::Inferred => hidden.clone(),
    })
}
