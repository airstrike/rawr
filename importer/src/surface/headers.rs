//! The headers of a crate's traits and trait impls (D604): what rawr reads
//! to fix a type parameter that only a trait's defaults and the crate's
//! impls decide, as the `Renderer` of `fn view(&self) -> impl
//! Widget<Message>` is. Each type is written in the facade's own types: a
//! crate type it declares, a parameterless alias of one that is the only
//! name of what it names, a type parameter, or a primitive. A header that
//! names any other type is left out.

use std::collections::{BTreeMap, BTreeSet};

use crate::api::{self, model};
use crate::classify::representability::{convert, signature};

/// A trait Rawr code names: its path, relative to the root, its identity,
/// and its type parameters other than `Self`, each with its default.
pub struct TraitHeader {
    pub path: Box<str>,
    pub identity: [u8; 32],
    pub parameters: Box<[signature::Parameter]>,
    pub defaults: Box<[Option<convert::Type>]>,
}

/// One impl of a trait, as `impl<W, M, T, R> Widget<M, T, R> for Column<W>
/// where W: Widget<M, T, R>`: its own type parameters, the trait's identity
/// and type arguments other than `Self`, the self type, and the
/// where-clause bounds that name a trait.
pub struct ImplHeader {
    pub parameters: Box<[signature::Parameter]>,
    pub trait_identity: [u8; 32],
    pub arguments: Box<[convert::Type]>,
    pub self_type: convert::Type,
    pub bounds: Box<[ImplBound]>,
}

/// A where-clause bound of an [`ImplHeader`]: `ty: Trait<arguments>`.
pub struct ImplBound {
    pub ty: convert::Type,
    pub trait_identity: [u8; 32],
    pub arguments: Box<[convert::Type]>,
}

/// The headers of `traits`, each a path relative to the root and the
/// trait's identity, and of every trait impl in `graph` whose types the
/// facade declaring `declared` can write.
pub fn headers<P: api::Phase>(
    graph: &api::Graph<P>,
    declared: &BTreeSet<[u8; 32]>,
    traits: &[(Box<str>, [u8; 32])],
) -> (Vec<TraitHeader>, Vec<ImplHeader>) {
    let headers = Headers::new(graph, declared);
    let mut trait_headers = traits
        .iter()
        .filter_map(|(path, identity)| headers.trait_header(path, identity))
        .collect::<Vec<_>>();
    trait_headers.sort_by(|left, right| left.path.cmp(&right.path));
    trait_headers.dedup_by(|left, right| left.path == right.path);
    let impls = graph
        .implementations()
        .filter_map(|implementation| headers.impl_header(implementation))
        .collect();
    (trait_headers, impls)
}

struct Headers<'g> {
    models: BTreeMap<[u8; 32], &'g model::Definition>,
    declared: &'g BTreeSet<[u8; 32]>,
    /// The declared parameterless aliases' targets, each with the alias's
    /// identity: the alias is that type's name (D604.5).
    aliases: Vec<(&'g model::Type, [u8; 32])>,
}

fn identity(id: &api::identity::Definition) -> [u8; 32] {
    *crate::surface::identity::Definition::of(id).as_bytes()
}

impl<'g> Headers<'g> {
    fn new<P: api::Phase>(graph: &'g api::Graph<P>, declared: &'g BTreeSet<[u8; 32]>) -> Self {
        let models = graph
            .definitions()
            .map(|model| (identity(model.id()), model))
            .collect();
        let aliases = graph
            .definitions()
            .filter_map(|definition| match definition.kind() {
                model::Kind::TypeAlias { ty, generics } if generics.parameters().len() == 0 => {
                    let alias = identity(definition.id());
                    declared.contains(&alias).then_some((ty, alias))
                }
                _ => None,
            })
            .collect();
        Self {
            models,
            declared,
            aliases,
        }
    }

    fn ty(&self, ty: &model::Type) -> Option<convert::Type> {
        if let Some((_, alias)) = self.aliases.iter().find(|(target, _)| *target == ty) {
            return Some(convert::Type::Nominal {
                definition: crate::surface::identity::Definition::authenticated(*alias),
                arguments: Box::default(),
            });
        }
        Some(match ty {
            model::Type::Tuple(members) if members.is_empty() => convert::Type::Unit,
            model::Type::Primitive(name) => {
                convert::Type::Builtin(convert::Builtin::primitive(name)?)
            }
            model::Type::Parameter { index, .. } => convert::Type::Parameter(*index),
            model::Type::Definition { id, arguments } => {
                let definition = identity(id);
                if !self.declared.contains(&definition) {
                    return None;
                }
                convert::Type::Nominal {
                    definition: crate::surface::identity::Definition::authenticated(definition),
                    arguments: arguments
                        .iter()
                        .filter_map(|argument| match argument {
                            model::Argument::Type(ty) => Some(self.ty(ty)),
                            _ => None,
                        })
                        .collect::<Option<_>>()?,
                }
            }
            _ => return None,
        })
    }

    /// A trait's type arguments other than `Self`, by its parameters.
    fn trait_arguments(
        &self,
        trait_id: &api::identity::Definition,
        arguments: &[model::Argument],
    ) -> Option<Box<[convert::Type]>> {
        let model::Kind::Trait { generics, .. } = self.models.get(&identity(trait_id))?.kind()
        else {
            return None;
        };
        // The parameters other than `Self` line up with the arguments'
        // tail, whether or not `Self` leads either list.
        let parameters = generics
            .parameters()
            .filter(|parameter| parameter.name() != "Self")
            .collect::<Vec<_>>();
        let offset = arguments.len().checked_sub(parameters.len())?;
        parameters
            .iter()
            .zip(&arguments[offset..])
            .filter(|(parameter, _)| {
                matches!(parameter.kind(), model::ParameterKind::Type { .. })
                    && parameter.name() != "Self"
            })
            .map(|(_, argument)| match argument {
                model::Argument::Type(ty) => self.ty(ty),
                _ => None,
            })
            .collect()
    }

    fn parameters(generics: &model::Generics) -> Box<[signature::Parameter]> {
        generics
            .parameters()
            .filter(|parameter| matches!(parameter.kind(), model::ParameterKind::Type { .. }))
            .map(|parameter| {
                signature::Parameter::new(
                    parameter.index(),
                    parameter.name().into(),
                    signature::Kind::Type,
                )
            })
            .collect()
    }

    fn trait_header(&self, path: &str, trait_identity: &[u8; 32]) -> Option<TraitHeader> {
        let model::Kind::Trait { generics, .. } = self.models.get(trait_identity)?.kind() else {
            return None;
        };
        let mut parameters = Vec::new();
        let mut defaults = Vec::new();
        for parameter in generics.parameters() {
            let model::ParameterKind::Type { default, .. } = parameter.kind() else {
                continue;
            };
            if parameter.name() == "Self" {
                continue;
            }
            parameters.push(signature::Parameter::new(
                parameter.index(),
                parameter.name().into(),
                signature::Kind::Type,
            ));
            defaults.push(match default {
                model::Default::Present(ty) => self.ty(ty),
                model::Default::Absent => None,
            });
        }
        Some(TraitHeader {
            path: path.into(),
            identity: *trait_identity,
            parameters: parameters.into_boxed_slice(),
            defaults: defaults.into_boxed_slice(),
        })
    }

    fn impl_header(&self, implementation: &model::Implementation) -> Option<ImplHeader> {
        if implementation.polarity() != model::Polarity::Positive {
            return None;
        }
        let model::Trait::Trait { id, arguments } = implementation.trait_ref() else {
            return None;
        };
        let generics = implementation.generics();
        let self_type = self.ty(implementation.self_type())?;
        let arguments = self.trait_arguments(id, arguments)?;
        let bounds = generics
            .predicates()
            .filter_map(|predicate| {
                let model::Predicate::Bound {
                    ty,
                    bound:
                        model::Bound::Trait {
                            trait_id,
                            arguments,
                            polarity: model::Polarity::Positive,
                            ..
                        },
                } = predicate
                else {
                    return None;
                };
                Some(ImplBound {
                    ty: self.ty(ty)?,
                    trait_identity: identity(trait_id),
                    arguments: self.trait_arguments(trait_id, arguments)?,
                })
            })
            .collect();
        Some(ImplHeader {
            parameters: Self::parameters(generics),
            trait_identity: identity(id),
            arguments,
            self_type,
            bounds,
        })
    }
}
