//! Opaque `impl Trait` types that a crate's functions give, as
//! `fn iced(..) -> impl Widget<Message, Theme, Renderer>` or
//! `fn application(..) -> Application<impl Program<..>>` do. Each is a type
//! of its own, named by its compiler definition: Rust code can't write it,
//! so a facade declares it under a name of its own and a runner reaches
//! its values only where the call that made one is compiled.

use std::collections::BTreeSet;

use crate::api::{self, model};

/// The identities of the opaque types the functions in `graph` give.
pub fn identities<P: api::Phase>(graph: &api::Graph<P>) -> BTreeSet<[u8; 32]> {
    let mut found = BTreeSet::new();
    for definition in graph.definitions() {
        if let model::Kind::Function { signature, .. } = definition.kind() {
            collect(signature.output(), &mut found);
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
        format!("__rawr_opaque_{name}")
    } else {
        format!("__rawr_opaque_{name}_{index}")
    };
    match parent {
        Some(parent) => format!("{parent}::{name}"),
        None => name,
    }
}
