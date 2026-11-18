//! The headers of a crate's traits and trait impls (D604): what rawr reads
//! to fix a type parameter that only a trait's defaults and the crate's
//! impls decide, as the `Renderer` of `fn view(&self) -> impl
//! Widget<Message>` is. Each type is written in the facade's own types: a
//! crate type it declares, a parameterless alias of one that is the only
//! name of what it names, a type parameter, or a primitive. A header that
//! names any other type is left out.
//!
//! Each header is written alone, a trait's, an impl's or the opaque types'
//! of one function, and notes what it reads ([`record`]): the graph's
//! entries, the definitions it looks up by semantic identity, and which
//! types its facade declares. So a header kept from an earlier plan is the
//! one a plan whose graph and facade read the same would write (#2222).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::api::{self, model};
use crate::classify::representability::{convert, signature};

/// A trait Rawr code names: its path, relative to the root, its identity,
/// its type parameters other than `Self`, each with its default, and the
/// inputs and output a callable fills it with through its one impl for
/// every callable (`classify::blanket_call`).
pub struct TraitHeader {
    pub path: Box<str>,
    pub identity: [u8; 32],
    pub parameters: Box<[signature::Parameter]>,
    pub defaults: Box<[Option<convert::Type>]>,
    pub call: Option<(Box<[convert::Type]>, convert::Type)>,
}

/// One impl of a trait, as `impl<W, M, T, R> Widget<M, T, R> for Column<W>
/// where W: Widget<M, T, R>`: its own type parameters, the trait's identity
/// and type arguments other than `Self`, the self type, the where-clause
/// bounds that name a trait, and those that fix an associated type.
pub struct ImplHeader {
    pub parameters: Box<[signature::Parameter]>,
    pub trait_identity: [u8; 32],
    pub arguments: Box<[convert::Type]>,
    pub self_type: convert::Type,
    pub bounds: Box<[ImplBound]>,
    pub projections: Box<[ImplProjection]>,
}

/// A where-clause bound of an [`ImplHeader`] that fixes an associated type,
/// as `Renderer: image::Renderer<Handle = Handle>` of iced's `impl Widget
/// for Image<Handle>`: `<ty as Trait>::Name` is `value`. Each case is a
/// type `ty` may be, with the associated type rustc's impls give it there,
/// so the bound fixes `value` once `ty` is known, as rustc's projection
/// does.
pub struct ImplProjection {
    pub ty: convert::Type,
    pub value: convert::Type,
    pub cases: Box<[(convert::Type, convert::Type)]>,
}

/// A where-clause bound of an [`ImplHeader`]: `ty: Trait<arguments>`.
pub struct ImplBound {
    pub ty: convert::Type,
    pub trait_identity: [u8; 32],
    pub arguments: Box<[convert::Type]>,
}

/// The headers of `traits`, each a path relative to the root and the
/// trait's identity, and of every trait impl in `graph` whose types the
/// facade declaring `declared` can write. `opaque` gives the number of
/// parameters the facade declares each opaque type with: an opaque type's
/// impl by its bounds is kept only where it writes the type at as many.
pub fn headers<P: api::Phase>(
    graph: &api::Graph<P>,
    declared: &BTreeSet<[u8; 32]>,
    opaque: &BTreeMap<[u8; 32], usize>,
    traits: &[(Box<str>, [u8; 32])],
) -> (Vec<TraitHeader>, Vec<ImplHeader>) {
    let models = Models::new(graph);
    let headers = models.facade(declared, opaque);
    let mut trait_headers = traits
        .iter()
        .filter_map(|(path, identity)| headers.trait_header(path, identity))
        .collect::<Vec<_>>();
    trait_headers.sort_by(|left, right| left.path.cmp(&right.path));
    trait_headers.dedup_by(|left, right| left.path == right.path);
    let mut impls = graph
        .implementations()
        .filter_map(|implementation| headers.impl_header(implementation))
        .collect::<Vec<_>>();
    for definition in graph.definitions() {
        impls.extend(headers.opaque_headers(definition));
    }
    (trait_headers, impls)
}

/// One read of a header's work beyond the graph's accessors: what the
/// graph's definitions give a semantic identity, and what the facade the
/// header is for declares (#2222).
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) enum Key {
    /// A read of the graph through its accessors.
    Graph(api::reads::Read),
    /// The definition whose semantic identity this is.
    Model(#[serde(with = "crate::hex")] [u8; 32]),
    /// Whether the facade declares the type of this identity.
    Declared(#[serde(with = "crate::hex")] [u8; 32]),
    /// How many parameters the facade declares this opaque type with.
    Opaque(#[serde(with = "crate::hex")] [u8; 32]),
    /// The facade's parameterless type aliases, each with what it names.
    Aliases,
    /// The crate type aliases that are the only name of their type.
    InstanceAliases,
}

thread_local! {
    static NOTED: RefCell<Vec<Key>> = const { RefCell::new(Vec::new()) };
}

/// Note that the header being written on this thread read `key`.
fn note(key: impl FnOnce() -> Key) {
    if api::reads::recording() {
        NOTED.with_borrow_mut(|noted| noted.push(key()));
    }
}

/// Run `work`, one header's of `graph`, and give the keys it read, each
/// once, in order.
pub(crate) fn record<T, P: api::Phase>(
    graph: &api::Graph<P>,
    work: impl FnOnce() -> T,
) -> (T, Vec<Key>) {
    let mut saved = Saved(Some(NOTED.take()));
    let (done, noted) = api::reads::record(work);
    let own = NOTED.replace(
        saved
            .0
            .take()
            .expect("the outer keys stay set aside while the work runs"),
    );
    let mut keys = noted
        .reads(graph)
        .into_iter()
        .map(Key::Graph)
        .chain(own)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.dedup();
    (done, keys)
}

/// The keys noted before a [`record`], put back when it ends, by return or
/// unwind.
struct Saved(Option<Vec<Key>>);

impl Drop for Saved {
    fn drop(&mut self) {
        if let Some(keys) = self.0.take() {
            NOTED.replace(keys);
        }
    }
}

/// What every facade's headers read of a graph by identity, read once for
/// a plan: each definition by its semantic identity, and each
/// parameterless type alias.
pub(crate) struct Models<'g, P: api::Phase> {
    graph: &'g api::Graph<P>,
    /// Each definition's index, by its semantic identity.
    models: BTreeMap<[u8; 32], usize>,
    /// Each parameterless type alias's target, with the alias's identity.
    aliases: Vec<(&'g model::Type, [u8; 32])>,
    /// The types only a crate's alias names (`classify::instance_aliases`).
    instances: std::cell::OnceCell<Box<[usize]>>,
}

impl<'g, P: api::Phase> Models<'g, P> {
    /// What `graph` gives every facade's headers. Its reads are no
    /// header's own: each header notes the identities it looks up.
    pub(crate) fn new(graph: &'g api::Graph<P>) -> Self {
        api::reads::paused(|| {
            let identities = graph.surface_definitions();
            let models = identities
                .iter()
                .enumerate()
                .map(|(index, identity)| (*identity.as_bytes(), index))
                .collect();
            let aliases = graph
                .definitions()
                .zip(identities)
                .filter_map(|(definition, identity)| match definition.kind() {
                    model::Kind::TypeAlias { ty, generics } if generics.parameters().len() == 0 => {
                        Some((ty, *identity.as_bytes()))
                    }
                    _ => None,
                })
                .collect();
            Self {
                graph,
                models,
                aliases,
                instances: std::cell::OnceCell::new(),
            }
        })
    }

    /// The headers of the facade that declares `declared`, whose opaque
    /// types `opaque` counts the parameters of.
    pub(crate) fn facade<'f>(
        &'f self,
        declared: &'f BTreeSet<[u8; 32]>,
        opaque: &'f BTreeMap<[u8; 32], usize>,
    ) -> Headers<'g, 'f, P> {
        Headers {
            models: self,
            declared,
            opaque,
            aliases: self
                .aliases
                .iter()
                .filter(|(_, alias)| declared.contains(alias))
                .copied()
                .collect(),
            aliases_digest: std::cell::OnceCell::new(),
        }
    }

    /// The definition whose semantic identity is `identity`, read by no
    /// header.
    pub(crate) fn id(&self, identity: &[u8; 32]) -> Option<&'g api::identity::Definition> {
        let index = *self.models.get(identity)?;
        api::reads::paused(|| self.graph.definition_at(index)).map(model::Definition::id)
    }

    fn instances(&self) -> &[usize] {
        note(|| Key::InstanceAliases);
        self.instances
            .get_or_init(|| api::reads::paused(|| crate::classify::instance_aliases(self.graph)))
    }
}

/// The headers of one facade: the traits it names, and the impls whose
/// types it can write. `opaque` gives the number of parameters the facade
/// declares each opaque type with: an opaque type's impl by its bounds is
/// kept only where it writes the type at as many.
pub(crate) struct Headers<'g, 'f, P: api::Phase> {
    models: &'f Models<'g, P>,
    declared: &'f BTreeSet<[u8; 32]>,
    opaque: &'f BTreeMap<[u8; 32], usize>,
    /// The declared parameterless aliases' targets, each with the alias's
    /// identity: the alias is that type's name (D604.5).
    aliases: Vec<(&'g model::Type, [u8; 32])>,
    /// The digest of [`Key::Aliases`], once asked.
    aliases_digest: std::cell::OnceCell<api::reads::Digest>,
}

fn identity(id: &api::identity::Definition) -> [u8; 32] {
    *crate::surface::identity::Definition::of(id).as_bytes()
}

/// A digest that stands for a yes, beside [`api::reads::ABSENT`] for a no.
const PRESENT: api::reads::Digest = [1; 32];

impl<'g, P: api::Phase> Headers<'g, '_, P> {
    /// What this facade and its graph give `key`.
    pub(crate) fn digest(&self, key: &Key) -> api::reads::Digest {
        let graph = self.models.graph;
        api::reads::paused(|| match key {
            Key::Graph(read) => graph.read_digest(read),
            Key::Model(identity) => self
                .models
                .models
                .get(identity)
                .map_or(api::reads::ABSENT, |index| graph.definition_digest(*index)),
            Key::Declared(identity) => {
                if self.declared.contains(identity) {
                    PRESENT
                } else {
                    api::reads::ABSENT
                }
            }
            Key::Opaque(identity) => match self.opaque.get(identity) {
                Some(count) => Sha256::digest((*count as u64).to_le_bytes()).into(),
                None => api::reads::ABSENT,
            },
            // Each alias's definition holds what it names.
            Key::Aliases => *self.aliases_digest.get_or_init(|| {
                let mut digest = Sha256::new();
                for (_, alias) in &self.aliases {
                    if let Some(index) = self.models.models.get(alias) {
                        digest.update(graph.definition_digest(*index));
                    }
                }
                digest.finalize().into()
            }),
            Key::InstanceAliases => {
                let mut digest = Sha256::new();
                for index in self.models.instances() {
                    digest.update(graph.definition_digest(*index));
                }
                digest.finalize().into()
            }
        })
    }

    /// The graph this facade's headers read.
    pub(crate) const fn graph(&self) -> &'g api::Graph<P> {
        self.models.graph
    }

    /// The definition whose semantic identity is `identity`.
    fn model(&self, identity: &[u8; 32]) -> Option<&'g model::Definition> {
        note(|| Key::Model(*identity));
        let index = *self.models.models.get(identity)?;
        api::reads::paused(|| self.models.graph.definition_at(index))
    }

    /// Whether the facade declares the type whose identity is `identity`.
    fn declares(&self, identity: &[u8; 32]) -> bool {
        note(|| Key::Declared(*identity));
        self.declared.contains(identity)
    }

    fn ty(&self, ty: &model::Type) -> Option<convert::Type> {
        note(|| Key::Aliases);
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
                // A std type Rawr holds as its own, as iced's `impl<T>
                // Widget for Option<T>` names `Option<T>`: the type the
                // classifier converts it to, which every facade can write.
                if let Some(model) = self.model(&definition)
                    && let Some(converted) = self.std_type(model, arguments)
                {
                    return converted;
                }
                if !self.declares(&definition) {
                    return None;
                }
                // The facade declares a type's type and const parameters, as
                // palette's `ArrayTable<const N: usize>`, and no lifetimes.
                // A const argument is one no facade type can write, as the
                // classifier refuses it in a signature.
                convert::Type::Nominal {
                    definition: crate::surface::identity::Definition::authenticated(definition),
                    arguments: arguments
                        .iter()
                        .filter_map(|argument| match argument {
                            model::Argument::Type(ty) => Some(self.ty(ty)),
                            model::Argument::Const(_) => Some(None),
                            model::Argument::Lifetime(_) => None,
                        })
                        .collect::<Option<_>>()?,
                }
            }
            _ => return None,
        })
    }

    /// `Option<T>`, `Result<T, E>` or `Vec<T>` of std, which Rawr holds as
    /// its own `Option`, `Result` and array: `Some` of the converted type,
    /// itself `None` when an argument can't be written. `None` for any
    /// other definition.
    fn std_type(
        &self,
        definition: &model::Definition,
        arguments: &[model::Argument],
    ) -> Option<Option<convert::Type>> {
        use crate::classify::{option_element, result_elements, std_binding, vec_element};
        // alloc's `String` is Rawr's string, as iced's `markdown::view`
        // gives an `impl Widget<Uri, ..>` whose `Uri` is a `String` (#917).
        if matches!(std_binding::of(definition), Some(std_binding::Std::String)) {
            return Some(Some(convert::Type::Builtin(convert::Builtin::String)));
        }
        // core's `fmt::Arguments<'_>`, as tracing-core's `impl Value for
        // fmt::Arguments<'_>` is for: the program's `format_args!` (#1561).
        if matches!(
            std_binding::of(definition),
            Some(std_binding::Std::Arguments)
        ) {
            return Some(Some(convert::Type::Builtin(convert::Builtin::Arguments)));
        }
        if let Some(element) = option_element(definition, arguments) {
            return Some(
                self.ty(element)
                    .map(|element| convert::Type::Option(Box::new(element))),
            );
        }
        if let Some((ok, err)) = result_elements(definition, arguments) {
            return Some(
                self.ty(ok)
                    .zip(self.ty(err))
                    .map(|(ok, err)| convert::Type::Result(Box::new(ok), Box::new(err))),
            );
        }
        let element = vec_element(definition, arguments)?;
        Some(self.ty(element).map(|element| convert::Type::Sequence {
            element: Box::new(element),
            owned: true,
            length: None,
        }))
    }

    /// A trait's type arguments other than `Self`, by its parameters.
    fn trait_arguments(
        &self,
        trait_id: &api::identity::Definition,
        arguments: &[model::Argument],
    ) -> Option<Box<[convert::Type]>> {
        let model::Kind::Trait { generics, .. } = self.model(&identity(trait_id))?.kind() else {
            return None;
        };
        // The parameters other than `Self` line up with the arguments'
        // tail, whether or not `Self` leads either list.
        let parameters = generics
            .parameters()
            .filter(|parameter| parameter.name() != "Self")
            .collect::<Vec<_>>();
        let offset = arguments.len().checked_sub(parameters.len())?;
        let pairs = parameters
            .iter()
            .zip(&arguments[offset..])
            .filter(|(parameter, _)| matches!(parameter.kind(), model::ParameterKind::Type { .. }))
            .collect::<Vec<_>>();
        let mut written = Vec::with_capacity(pairs.len());
        for (position, (_, argument)) in pairs.iter().enumerate() {
            let model::Argument::Type(ty) = argument else {
                return None;
            };
            match self.ty(ty) {
                Some(ty) => written.push(ty),
                // A type the facade can't write ends the arguments when it
                // and each one after it is the trait's default at the
                // arguments before it, as a bound written `impl
                // Widget<Message>` leaves them: rawr fills them in from
                // the trait's header, as rustc does (#1804).
                None if pairs[position..]
                    .iter()
                    .all(|(parameter, argument)| defaulted(parameter, argument, &pairs)) =>
                {
                    break;
                }
                None => return None,
            }
        }
        Some(written.into_boxed_slice())
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

    pub fn trait_header(&self, path: &str, trait_identity: &[u8; 32]) -> Option<TraitHeader> {
        let definition = self.model(trait_identity)?;
        let model::Kind::Trait { generics, .. } = definition.kind() else {
            return None;
        };
        let mut parameters = Vec::new();
        let mut defaults = Vec::new();
        let mut own = Vec::new();
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
            own.push(model::Argument::Type(model::Type::Parameter {
                owner: definition.id().clone(),
                index: parameter.index(),
            }));
        }
        // The call at the trait's own parameters. An input may be a borrow,
        // as `Fn(Input, &Theme)`'s is.
        let call = crate::classify::blanket_call(self.models.graph, definition.id(), &own)
            .and_then(|(inputs, output)| {
                let input = |ty: &model::Type| match ty {
                    model::Type::Reference { mutability, ty, .. } => {
                        Some(convert::Type::Borrowed {
                            mutable: *mutability == model::Mutability::Mutable,
                            ty: Box::new(self.ty(ty)?),
                        })
                    }
                    ty => self.ty(ty),
                };
                let inputs = inputs.iter().map(input).collect::<Option<Box<[_]>>>()?;
                Some((inputs, self.ty(&output)?))
            });
        Some(TraitHeader {
            path: path.into(),
            identity: *trait_identity,
            parameters: parameters.into_boxed_slice(),
            defaults: defaults.into_boxed_slice(),
            call,
        })
    }

    /// The impls an opaque type a function gives has by its bounds, as
    /// `fn opaque<Message, Theme, Renderer>(..) -> impl Widget<Message,
    /// Theme, Renderer>` gives a type that is `Widget<Message, Theme,
    /// Renderer>` at the arguments it captures. Rustc reads an opaque
    /// type's traits from its bounds alone, so the resolver selects these
    /// as any other impl. The self type is the opaque type at every type it
    /// captures. One the facade declares with other parameters (a capture
    /// left out, an associated type's value) has no such header.
    pub fn opaque_headers(&self, definition: &model::Definition) -> Vec<ImplHeader> {
        let model::Kind::Function {
            signature,
            generics,
        } = definition.kind()
        else {
            return Vec::new();
        };
        let mut opaques = Vec::new();
        crate::classify::impl_traits(signature.output(), &mut opaques);
        let mut headers = Vec::new();
        for opaque in opaques {
            let model::Type::ImplTrait {
                id,
                bounds,
                captures,
            } = opaque
            else {
                continue;
            };
            let opaque = identity(id);
            // A trait's method captures the trait's `Self`, its parameter 0,
            // as iced's `Widget::explain` gives `impl Widget` of the widget
            // it is called on.
            let Some(arguments) = captures
                .iter()
                .filter_map(|capture| match capture {
                    model::Argument::Type(model::Type::SelfType(owner))
                        if self.model(&identity(owner)).is_some_and(|model| {
                            matches!(model.kind(), model::Kind::Trait { .. })
                        }) =>
                    {
                        Some(Some(convert::Type::Parameter(0)))
                    }
                    model::Argument::Type(ty) => Some(self.ty(ty)),
                    _ => None,
                })
                .collect::<Option<Box<[_]>>>()
            else {
                continue;
            };
            note(|| Key::Opaque(opaque));
            if self.opaque.get(&opaque) != Some(&arguments.len()) {
                continue;
            }
            // The header's parameters are those the captures name, which
            // may be an enclosing impl's as well as the function's own.
            let mut named = BTreeSet::new();
            for argument in arguments.iter() {
                parameter_indices(argument, &mut named);
            }
            // An `impl Trait` input's parameter has no name a path can
            // spell, and one of an enclosing impl isn't among `generics`:
            // each is named as a signature names an argument's `impl
            // Trait`, which no crate's own parameter is.
            let parameters = named
                .iter()
                .map(|index| {
                    let name = generics
                        .parameters()
                        .find(|parameter| parameter.index() == *index)
                        .map(|parameter| parameter.name())
                        .filter(|name| {
                            !name.is_empty()
                                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                                && !name.starts_with(|c: char| c.is_ascii_digit())
                        })
                        .map_or_else(|| signature::argument_impl_name(*index), Into::into);
                    signature::Parameter::new(*index, name, signature::Kind::Type)
                })
                .collect::<Box<[_]>>();
            let self_type = convert::Type::Nominal {
                definition: crate::surface::identity::Definition::authenticated(opaque),
                arguments,
            };
            for (trait_id, arguments) in self.elaborate(bounds) {
                let Some(arguments) = self.trait_arguments(&trait_id, &arguments) else {
                    continue;
                };
                let mut bound = BTreeSet::new();
                for argument in arguments.iter() {
                    parameter_indices(argument, &mut bound);
                }
                if !bound.is_subset(&named) {
                    continue;
                }
                headers.push(ImplHeader {
                    parameters: parameters.clone(),
                    trait_identity: identity(&trait_id),
                    arguments,
                    self_type: self_type.clone(),
                    bounds: Box::default(),
                    projections: Box::default(),
                });
            }
        }
        headers
    }

    /// Each trait an opaque type's `bounds` name, at its arguments, then
    /// each of their supertraits at the arguments the bound gives the
    /// trait's parameters, as rustc elaborates an opaque type's bounds:
    /// `impl Framed<Message, Theme, Renderer>` is a `Widget<Message,
    /// Theme, Renderer>` too when `Widget` is a supertrait of `Framed`.
    fn elaborate(
        &self,
        bounds: &[model::Predicate],
    ) -> Vec<(api::identity::Definition, Box<[model::Argument]>)> {
        /// Supertraits are read this deep.
        const DEPTH: usize = 8;
        let is_self = |ty: &model::Type| {
            matches!(
                ty,
                model::Type::SelfType(_) | model::Type::Parameter { index: 0, .. }
            )
        };
        let mut pending = Vec::new();
        let read = |predicates: &[model::Predicate],
                    bindings: &BTreeMap<u32, model::Type>,
                    depth: usize,
                    pending: &mut Vec<_>| {
            for predicate in predicates {
                let model::Predicate::Bound {
                    ty,
                    bound:
                        model::Bound::Trait {
                            trait_id,
                            arguments,
                            polarity: model::Polarity::Positive,
                            binders,
                        },
                } = predicate
                else {
                    continue;
                };
                if !binders.is_empty() || (depth != 0 && !is_self(ty)) {
                    continue;
                }
                let arguments = arguments
                    .iter()
                    .map(|argument| match argument {
                        model::Argument::Type(ty) => model::Argument::Type(
                            crate::classify::substitute_bindings(ty, bindings),
                        ),
                        other => other.clone(),
                    })
                    .collect::<Box<[_]>>();
                pending.push((trait_id.clone(), arguments, depth));
            }
        };
        read(bounds, &BTreeMap::new(), 0, &mut pending);
        let mut seen = Vec::<(api::identity::Definition, Box<[model::Argument]>)>::new();
        while let Some((trait_id, arguments, depth)) = pending.pop() {
            if depth >= DEPTH
                || seen
                    .iter()
                    .any(|(known, given)| *known == trait_id && *given == arguments)
            {
                continue;
            }
            if let Some(model::Kind::Trait {
                supertraits,
                generics,
                ..
            }) = self
                .model(&identity(&trait_id))
                .map(|definition| definition.kind())
            {
                // The trait's parameters after `Self` take the bound's
                // arguments after `Self`, in order.
                let parameters = generics
                    .parameters()
                    .filter(|parameter| parameter.index() != 0)
                    .collect::<Vec<_>>();
                let offset = arguments.len().saturating_sub(parameters.len());
                let bindings = parameters
                    .iter()
                    .zip(&arguments[offset..])
                    .filter_map(|(parameter, argument)| match argument {
                        model::Argument::Type(ty) => Some((parameter.index(), ty.clone())),
                        _ => None,
                    })
                    .collect();
                read(supertraits, &bindings, depth + 1, &mut pending);
            }
            seen.push((trait_id, arguments));
        }
        seen
    }

    pub fn impl_header(&self, implementation: &model::Implementation) -> Option<ImplHeader> {
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
        let projections = generics
            .predicates()
            .filter_map(|predicate| match predicate {
                model::Predicate::ProjectionEquality { projection, ty } => {
                    self.impl_projection(projection, ty)
                }
                _ => None,
            })
            .collect();
        Some(ImplHeader {
            parameters: Self::parameters(generics),
            trait_identity: identity(id),
            arguments,
            self_type,
            bounds,
            projections,
        })
    }

    /// The where-clause bound `<this as Trait>::Name == value`, with a case
    /// for each type the facade names that `this` may be: each of the
    /// trait's impls for a type it declares, and each type only a crate's
    /// alias names, at the associated type rustc's impls give it there.
    /// `None` for a trait that takes arguments besides `Self`, or a bound
    /// whose types the facade can't write.
    fn impl_projection(
        &self,
        projection: &model::Type,
        value: &model::Type,
    ) -> Option<ImplProjection> {
        let model::Type::Projection {
            trait_id,
            associated,
            arguments,
        } = projection
        else {
            return None;
        };
        let [model::Argument::Type(this)] = arguments
            .iter()
            .filter(|argument| !matches!(argument, model::Argument::Lifetime(_)))
            .collect::<Vec<_>>()[..]
        else {
            return None;
        };
        let name = self.model(&identity(associated))?.display_path().last()?;
        let candidates = crate::classify::projection_candidates(
            self.models.graph,
            trait_id,
            self.models.instances(),
        );
        let mut cases = Vec::new();
        for candidate in &candidates {
            let Some(at) =
                crate::classify::associated_at(self.models.graph, trait_id, name, candidate, 0)
            else {
                continue;
            };
            let (Some(case), Some(at)) = (self.ty(candidate), self.ty(&at)) else {
                continue;
            };
            if !cases.iter().any(|(known, _)| *known == case) {
                cases.push((case, at));
            }
        }
        Some(ImplProjection {
            ty: self.ty(this)?,
            value: self.ty(value)?,
            cases: cases.into_boxed_slice(),
        })
    }
}

/// Whether `argument` is `parameter`'s default, the trait's parameters in
/// it at the arguments `pairs` gives them, as `Renderer` is in a bound
/// written `impl Widget<Message>`.
fn defaulted(
    parameter: &model::Parameter,
    argument: &model::Argument,
    pairs: &[(&&model::Parameter, &model::Argument)],
) -> bool {
    let (
        model::ParameterKind::Type {
            default: model::Default::Present(default),
            ..
        },
        model::Argument::Type(argument),
    ) = (parameter.kind(), argument)
    else {
        return false;
    };
    let bindings = pairs
        .iter()
        .filter_map(|(parameter, argument)| match argument {
            model::Argument::Type(ty) => Some((parameter.index(), ty.clone())),
            _ => None,
        })
        .collect();
    crate::classify::substitute_bindings(default, &bindings) == *argument
}

/// The indices of the type parameters `ty` names.
fn parameter_indices(ty: &convert::Type, found: &mut BTreeSet<u32>) {
    match ty {
        convert::Type::Parameter(index) => {
            found.insert(*index);
        }
        convert::Type::Nominal { arguments, .. }
        | convert::Type::Tuple(arguments)
        | convert::Type::Script { arguments, .. } => {
            for argument in arguments.iter() {
                parameter_indices(argument, found);
            }
        }
        convert::Type::Option(inner)
        | convert::Type::Sequence { element: inner, .. }
        | convert::Type::Borrowed { ty: inner, .. }
        | convert::Type::Range(inner)
        | convert::Type::ProfileMessage { payload: inner, .. } => parameter_indices(inner, found),
        convert::Type::ConstArray { element, parameter } => {
            found.insert(*parameter);
            parameter_indices(element, found);
        }
        convert::Type::Result(ok, err)
        | convert::Type::FunctionItem {
            input: ok,
            output: err,
        } => {
            parameter_indices(ok, found);
            parameter_indices(err, found);
        }
        convert::Type::Object(object) => {
            for ty in object.types() {
                parameter_indices(ty, found);
            }
        }
        convert::Type::Unit
        | convert::Type::Never
        | convert::Type::Builtin(_)
        | convert::Type::Constant(_)
        | convert::Type::Inferred => {}
    }
}
