//! The program's and the crates' models: the lowering tables every parser
//! shares, and each nominal type's model.

use super::inference::AssociatedTypes;
use super::ty::Ty;
use super::{
    function_name, inherit_names, instance_text, is_iterator_trait, item_generics, lex_rust,
    mentions_parameter, sequence, source_span, trait_objects, use_blocks, ImplArguments,
};
use crate::resolve::{item, type_ref, Program};
use crate::{capability, method, type_def, Engine, ImmutableString, Position, Shared, Span};
use std::collections::{BTreeMap, BTreeSet};

/// One source context over the program-wide lowering tables, which every
/// function, method and closure parser shares rather than copies.
#[derive(Clone)]
pub(super) struct Models {
    pub(super) context: crate::program::source::Id,
    pub(super) tables: Shared<Tables>,
    /// The first segment of a path being read that starts with `::`
    /// (`::image::ImageReader`): an outside crate, which no `use` shadows.
    pub(super) global_head: Option<ImmutableString>,
    /// Where the lowering is in the source, which says whether a name a
    /// block's `use` binds is in scope; `NONE` before any token is read.
    pub(super) at: Position,
    /// Native closure copies retain the original whole-candidate contribution.
    #[cfg(feature = "rust-import")]
    pub(super) _imported_owner: Option<crate::package::import::runner::account::support::Owner>,
    #[cfg(test)]
    pub(super) _meter: Meter,
}

/// The program-wide lowering tables, built once per lowering.
pub(super) struct Tables {
    /// Each type's `IndexMut::index_mut` whose body is `&mut <place>`: its
    /// parameter's name and its body's text, inlined where the type is
    /// indexed (`indexed_place`).
    pub(super) index_mut: BTreeMap<type_def::Id, (ImmutableString, String)>,
    pub(super) runtime: crate::program::model::Model,
    pub(super) host: Shared<crate::host::Registry>,
    /// What the program may reach while it compiles: the files
    /// `include_str!` reads and the variables `env!` reads (D683.4).
    pub(super) access: crate::Access,
    /// Whether the program builds as `rustc --test` builds it: `cfg(test)`
    /// holds.
    pub(super) test: bool,
    /// The features Cargo enables for the package: `cfg(feature = "..")`
    /// holds for each.
    pub(super) features: crate::project::cargo::features::Enabled,
    pub(super) imports:
        BTreeMap<(crate::program::source::Id, ImmutableString), crate::resolve::item::Binding>,
    /// Each source's original imports and their lexical scopes (D652).
    pub(super) lexical: BTreeMap<crate::program::source::Id, Box<[crate::item::Use]>>,
    /// The blocks a name is in scope in when only blocks' `use`s bind it
    /// in its source: rustc scopes a block's `use` to the block (#7).
    pub(super) block_scopes: BTreeMap<(crate::program::source::Id, ImmutableString), Box<[Span]>>,
    pub(super) by_name: BTreeMap<(crate::program::source::Id, ImmutableString), Nominal>,
    /// The source that declares each script type.
    pub(super) declared_in: BTreeMap<type_def::Id, crate::program::source::Id>,
    /// The enum variants each source imports by name (`use Shape::*;`), with
    /// the name of their enum in that source.
    pub(super) variants:
        BTreeMap<(crate::program::source::Id, ImmutableString), Box<[ImmutableString]>>,
    /// The crate enums' variants each source imports by name (`use
    /// iced::Center;`), each as the hidden name its enum is imported under
    /// and the variant's own name, which a renamed import keeps.
    pub(super) host_variants:
        BTreeMap<(crate::program::source::Id, ImmutableString), Box<[ImmutableString]>>,
    pub(super) by_definition: BTreeMap<type_def::Id, Nominal>,
    pub(super) by_identity: BTreeMap<type_def::Identity, Nominal>,
    pub(super) qualified_functions: BTreeMap<(crate::program::source::Id, Span), crate::item::Key>,
    pub(super) qualified_owners: BTreeMap<(crate::program::source::Id, Span), type_def::Identity>,
    pub(super) qualified_pattern_owners:
        BTreeMap<(crate::program::source::Id, Span), type_def::Identity>,
    pub(super) qualified_rust: BTreeMap<(crate::program::source::Id, Span), Box<[ImmutableString]>>,
    /// The name each source writes a crate trait by, when another module's
    /// trait has that name too, with the path the trait is known by.
    trait_names: BTreeMap<(crate::program::source::Id, ImmutableString), ImmutableString>,
    /// The app crate whose shadow holds an async `main`'s entry (D513.4).
    pub(super) entry_crate: Option<ImmutableString>,
    /// The marked types the runner holds as their mirrors in rawr's copy
    /// of the crate, by package and path (#1249).
    #[cfg(feature = "rust-import")]
    pub(super) mirrors: BTreeSet<Box<[ImmutableString]>>,
    /// Each marked type by its mirror's identity, which the native crate's
    /// signatures name it by: `fn base(theme: &Theme)` in a native module
    /// takes the marked `Theme`.
    #[cfg(feature = "rust-import")]
    pub(super) mirrored: std::sync::Arc<super::ty::Mirrored>,
    /// The crates the program's paths may start with (`Program::crates`).
    pub(super) crates: Option<crate::resolve::extern_prelude::Prelude>,
    /// The type aliases each source names, resolved once with the headers.
    pub(super) aliases: BTreeMap<(crate::program::source::Id, ImmutableString), type_ref::Alias>,
    /// The module each source is, which decides what it sees of another
    /// module's private items.
    pub(super) namespaces: BTreeMap<crate::program::source::Id, crate::resolve::Namespace>,
    pub(super) methods: BTreeMap<(type_def::Id, ImmutableString), method::Definition>,
    /// Each type's `impl From<T>` functions, by the type converted to.
    pub(super) conversions: BTreeMap<type_def::Id, Vec<method::Definition>>,
    /// Functions of `impl Trait for Owner` blocks (and the trait defaults
    /// each such impl receives), by owner and function name.
    pub(super) trait_methods: BTreeMap<(type_def::Id, ImmutableString), Vec<method::Definition>>,
    /// The crate's trait declarations by name, with their sources.
    pub(super) traits: BTreeMap<ImmutableString, (crate::program::source::Id, item::Trait)>,
    /// `(trait, owner)` for each `impl Trait for Owner`.
    pub(super) impls: BTreeSet<(ImmutableString, type_def::Id)>,
    /// The crate types with an impl written for their `'static` instance
    /// (`impl Copy for Foo<'static>`), whose borrows the checker leaves to
    /// rustc.
    pub(super) static_impls: BTreeSet<type_def::Id>,
    /// The trait arguments of each `impl Trait<A> for Owner` of a generic
    /// trait, with the impl's own parameters.
    pub(super) impl_arguments: BTreeMap<(ImmutableString, type_def::Id), ImplArguments>,
    /// The distinct argument lists without parameters of each generic
    /// trait's impls: one dispatcher per instance.
    pub(super) trait_instances: BTreeMap<ImmutableString, Vec<Box<[type_ref::Type]>>>,
    /// `type Name = T;` of each `impl Trait for Owner`.
    pub(super) associated: Shared<AssociatedTypes>,
    /// The std traits among `traits` (`Clone`, `Display`, ..).
    pub(super) std_traits: BTreeSet<ImmutableString>,
    pub(super) functions: BTreeMap<(crate::program::source::Id, ImmutableString), ScriptFunction>,
    pub(super) functions_by_key: BTreeMap<crate::item::Key, ScriptFunction>,
    pub(super) sources: crate::program::source::Table,
    /// The program's root file: the crate root Cargo built the target from.
    pub(super) entry: crate::program::source::Id,
    /// The generic functions compiled per instantiation (D629).
    pub(super) instances: crate::resolve::Instances,
    /// The script's impls of crate traits for its own types, which a
    /// crate's bound selects as rustc selects an impl (D627, D629).
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(super) script_impls: Vec<ScriptTraitImpl>,
    /// The program's own `macro_rules!` macros and their scopes.
    pub(super) macros: crate::resolve::macros::Table,
    /// Each of `macros`' rules, by its source and span, read once.
    pub(super) macro_rules: BTreeMap<
        (crate::program::source::Id, Span),
        Result<Shared<crate::resolve::macro_rules::Rules>, crate::resolve::macro_rules::Malformed>,
    >,
}

/// A script's impl of a crate's trait for a script type (`Tables::
/// script_impls`): `impl<R: renderer::Renderer> Widget<M, T, R> for Quad`.
#[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
#[derive(Clone)]
pub(super) struct ScriptTraitImpl {
    /// The impl item's index.
    pub(super) index: usize,
    /// The trait's path from its crate's root.
    pub(super) path: ImmutableString,
    /// The impl's own type parameters.
    pub(super) generics: Box<[ImmutableString]>,
    /// The type it is for.
    pub(super) owner: type_ref::Type,
    /// The trait's arguments as the header writes them.
    pub(super) arguments: Box<[type_ref::Type]>,
}

impl std::ops::Deref for Models {
    type Target = Tables;

    fn deref(&self) -> &Tables {
        &self.tables
    }
}

#[cfg(test)]
std::thread_local! {
    static MODEL_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) struct Meter;

#[cfg(test)]
impl Clone for Meter {
    fn clone(&self) -> Self {
        MODEL_CLONES.with(|clones| clones.set(clones.get() + 1));
        Self
    }
}

#[cfg(test)]
pub(super) fn reset_model_clones() {
    MODEL_CLONES.with(|clones| clones.set(0));
}

#[cfg(test)]
pub(super) fn model_clones() -> usize {
    MODEL_CLONES.with(std::cell::Cell::get)
}

#[derive(Clone)]
pub(super) struct ScriptFunction {
    pub(super) key: crate::item::Key,
    /// The file that defines it.
    pub(super) source: crate::program::source::Id,
    pub(super) definition: item::Function,
    pub(super) generated: ImmutableString,
}

#[derive(Clone)]
pub(super) enum Nominal {
    Struct {
        definition: type_def::Definition,
        item: item::Struct,
    },
    Enum {
        definition: type_def::Definition,
        item: item::Enum,
    },
}

impl Models {
    pub(super) fn new(
        program: &Program,
        runtime: crate::program::model::Model,
        host: Shared<crate::host::Registry>,
        engine: &Engine,
    ) -> Self {
        let definitions: BTreeMap<_, _> = program
            .types
            .iter()
            .cloned()
            .map(|definition| (definition.id(), definition))
            .collect();
        let mut by_name = BTreeMap::new();
        let mut declared_in = BTreeMap::new();
        let mut by_definition = BTreeMap::new();
        let mut by_identity = BTreeMap::new();
        let methods = program
            .methods
            .iter()
            .filter(|method| method.conversion().is_none() && method.trait_name().is_none())
            .cloned()
            .map(|method| ((method.owner(), method.name().into()), method))
            .collect();
        let mut trait_methods =
            BTreeMap::<(type_def::Id, ImmutableString), Vec<method::Definition>>::new();
        let mut impls = BTreeSet::new();
        for method in program.methods.iter().chain(&program.builtin_methods) {
            if let Some(trait_name) = method.trait_name() {
                impls.insert((trait_name.clone(), method.owner()));
                trait_methods
                    .entry((method.owner(), method.name().into()))
                    .or_default()
                    .push(method.clone());
            }
        }
        let traits = program
            .items
            .iter()
            .filter_map(|shell| match &shell.kind {
                item::Kind::Trait(declared) => {
                    Some((declared.name.text.clone(), (shell.source, declared.clone())))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        // The std traits the crate implements by hand, unless it declares
        // its own of that name.
        let mut traits = traits;
        let mut std_traits = BTreeSet::new();
        let bounds = program
            .items
            .iter()
            .flat_map(|shell| item_generics(&shell.kind))
            .flat_map(|generic| generic.traits.iter())
            // `type Item: Into<String>;` bounds by a prelude trait too.
            .chain(program.items.iter().flat_map(|shell| {
                match &shell.kind {
                    item::Kind::Trait(declared) => declared
                        .associated_bounds
                        .iter()
                        .flat_map(|(_, traits)| traits.iter())
                        .collect::<Vec<_>>(),
                    _ => Vec::new(),
                }
            }))
            .map(|name| name.text.clone())
            .collect::<BTreeSet<_>>();
        // The std traits the crate implements or bounds by, with their
        // supertraits (an `Ord` bound reaches `PartialOrd`'s `partial_cmp`).
        let mut used = impls
            .iter()
            .map(|(trait_name, _)| trait_name.clone())
            .chain(
                bounds
                    .into_iter()
                    .filter(|name| program.std_traits().contains_key(name)),
            )
            .collect::<BTreeSet<_>>();
        let mut pending = used.iter().cloned().collect::<Vec<_>>();
        while let Some(name) = pending.pop() {
            let Some(declared) = program.std_traits().get(&name) else {
                continue;
            };
            for parent in declared.supertraits.1.iter() {
                if used.insert(parent.text.clone()) {
                    pending.push(parent.text.clone());
                }
            }
        }
        let mut declarations = program.std_traits().clone();
        // std's `Error`, which `Box<dyn Error>` names without an impl, and
        // each std trait a body names a trait object of (`Vec<Box<dyn
        // Display>>`), as the program's scope has it.
        let uses = crate::resolve::std_trait::uses(&program.items);
        let objects = trait_objects(engine, &program.source);
        let named = objects.iter().filter_map(|segments| {
            let segments = segments.iter().map(String::as_str).collect::<Vec<_>>();
            let name = *segments.last()?;
            // `dyn Iterator<Item = T>` is rawr's own iterator (`dyn_type`).
            if is_iterator_trait(name) {
                return None;
            }
            let definition = match &segments[..] {
                [_] => crate::resolve::std_trait::named(
                    name,
                    uses.iter()
                        .map(|(binding, qualified)| (binding.as_str(), qualified.as_str())),
                )?,
                _ => crate::resolve::std_trait::by_path(&segments)?,
            };
            Some((ImmutableString::from(name), Some(definition)))
        });
        for (name, definition) in
            std::iter::once((ImmutableString::from("Error"), None)).chain(named)
        {
            if traits.contains_key(&name) {
                continue;
            }
            if !declarations.contains_key(&name)
                && let Some(declared) = definition
                    .or_else(|| crate::resolve::std_trait::anywhere(&name))
                    .and_then(|definition| {
                        crate::resolve::std_trait::declaration(definition, &[], &host)
                    })
            {
                declarations.insert(name.clone(), declared);
            }
            used.insert(name);
        }
        for trait_name in &used {
            if !traits.contains_key(trait_name)
                && let Some(declared) = declarations.get(trait_name)
            {
                traits.insert(
                    trait_name.clone(),
                    (crate::program::source::Id::ROOT, declared.clone()),
                );
                // A std trait whose methods rawr's runtime runs for its own
                // values falls back to them; another dispatches as a crate
                // trait does.
                if declared
                    .std
                    .is_some_and(crate::resolve::std_trait::runs_builtin)
                {
                    std_traits.insert(trait_name.clone());
                }
            }
        }
        let mut conversions = BTreeMap::<type_def::Id, Vec<method::Definition>>::new();
        for method in program
            .methods
            .iter()
            .chain(&program.builtin_methods)
            // A conversion compiled per instantiation is chosen as its
            // template, which binds its instance (D629).
            .filter(|method| {
                method.conversion().is_some() && !program.instances.types.contains_key(method.key())
            })
        {
            conversions
                .entry(method.owner())
                .or_default()
                .push(method.clone());
        }
        let mut functions = BTreeMap::new();
        let mut functions_by_key = BTreeMap::new();
        let mut imports = BTreeMap::<_, item::Binding>::new();

        for shell in program.items.iter() {
            let nominal = match &shell.kind {
                item::Kind::Struct(item) => Nominal::Struct {
                    definition: definitions[&item.definition].clone(),
                    item: item.clone(),
                },
                item::Kind::Enum(item) => Nominal::Enum {
                    definition: definitions[&item.definition].clone(),
                    item: item.clone(),
                },
                item::Kind::Use(_)
                | item::Kind::Impl(_)
                | item::Kind::Trait(_)
                | item::Kind::Function(_) => continue,
            };
            by_name.insert((shell.source, nominal.name().into()), nominal.clone());
            declared_in.insert(nominal.definition().id(), shell.source);
            by_definition.insert(nominal.definition().id(), nominal.clone());
            by_identity.insert(nominal.identity(), nominal);
        }

        for shell in program.items.iter() {
            if let item::Kind::Function(function) = &shell.kind {
                let key = shell
                    .key
                    .clone()
                    .expect("resolved functions retain stable item keys");
                let script = ScriptFunction {
                    key: key.clone(),
                    source: shell.source,
                    definition: function.clone(),
                    generated: function_name(shell.id).into(),
                };
                functions.insert((shell.source, function.name.text.clone()), script.clone());
                functions_by_key.insert(key, script);
            }
        }

        let mut imported_variants = Vec::new();
        let mut ordering_variants = Vec::new();
        let mut host_variants = Vec::new();
        // A name only blocks' `use`s bind is in scope in those blocks; one a
        // module's `use` or an item of the source binds is in scope there.
        let declared = by_name
            .keys()
            .chain(functions.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut block_scopes = BTreeMap::<_, Vec<Span>>::new();
        let mut module_names = BTreeSet::new();
        for shell in program.items.iter() {
            let item::Kind::Use(item) = &shell.kind else {
                continue;
            };
            let blocks = program
                .lexical
                .get(&shell.source)
                .and_then(|imports| use_blocks(imports, item.span));
            for binding in &item.bindings {
                let key = (shell.source, binding.name.text.clone());
                match &blocks {
                    Some(blocks) if !declared.contains(&key) => {
                        block_scopes.entry(key).or_default().extend(blocks);
                    }
                    _ => {
                        module_names.insert(key);
                    }
                }
            }
            for binding in &item.bindings {
                // A module and a function of its name, bound in their own
                // namespaces, read as one name.
                let key = (shell.source, binding.name.text.clone());
                let joined = match imports.remove(&key) {
                    Some(previous) if previous.target.namespace() != binding.target.namespace() => {
                        item::Binding::join(previous, binding.clone())
                    }
                    _ => binding.clone(),
                };
                imports.insert(key, joined);
                match &binding.target {
                    item::Import::ScriptType { identity, .. } => {
                        let nominal = by_identity
                            .get(identity)
                            .expect("resolved script imports retain nominal identity")
                            .clone();
                        by_name.insert((shell.source, binding.name.text.clone()), nominal);
                    }
                    item::Import::ScriptItem { key } => {
                        let function = functions_by_key
                            .get(key)
                            .expect("resolved script item imports retain callable identity")
                            .clone();
                        functions.insert((shell.source, binding.name.text.clone()), function);
                    }
                    item::Import::Variant { identity, .. } => {
                        imported_variants.push((
                            shell.source,
                            binding.name.text.clone(),
                            identity.clone(),
                        ));
                    }
                    item::Import::OrderingVariant => {
                        ordering_variants.push((shell.source, binding.name.text.clone()));
                    }
                    item::Import::HostVariant { owner } => {
                        host_variants.push((
                            shell.source,
                            binding.name.text.clone(),
                            owner.clone(),
                            binding.qualified.clone(),
                        ));
                    }
                    item::Import::Module
                    | item::Import::ModuleItem { .. }
                    | item::Import::Type { .. }
                    | item::Import::Item { .. }
                    | item::Import::Alias
                    | item::Import::Derive => {}
                }
            }
        }
        let block_scopes = block_scopes
            .into_iter()
            .filter(|(key, _)| !module_names.contains(key))
            .map(|(key, blocks)| (key, blocks.into_boxed_slice()))
            .collect();
        // An imported variant reads as its enum's path, `Shape::Circle`. The
        // enum is named by its own name in that source, which a different
        // type of that name there leaves to a name no source can write.
        let mut variants = BTreeMap::new();
        for (source, name, identity) in imported_variants {
            let nominal = by_identity
                .get(&identity)
                .expect("resolved variant imports retain nominal identity")
                .clone();
            let own: ImmutableString = nominal.name().into();
            let owner = match by_name.get(&(source, own.clone())) {
                Some(named) if named.identity() == nominal.identity() => own,
                Some(_) => {
                    let hidden: ImmutableString = format!("{own}\u{2}").into();
                    by_name.insert((source, hidden.clone()), nominal);
                    hidden
                }
                None => {
                    by_name.insert((source, own.clone()), nominal);
                    own
                }
            };
            variants.insert((source, name), Box::from([owner]));
        }
        for key in ordering_variants {
            variants.insert(
                key,
                Box::from(["std".into(), "cmp".into(), "Ordering".into()]),
            );
        }
        // A crate enum's variant reads as its enum's variant,
        // `Alignment::Center`, with the enum imported under a name no source
        // can write. A renamed import keeps the variant's own name.
        let mut variant_owners =
            BTreeMap::<(crate::program::source::Id, ImmutableString), ImmutableString>::new();
        let mut host_variants_by_name = BTreeMap::new();
        for (source, name, owner, qualified) in host_variants {
            let Some(crate::host::Entry::Type(definition)) = host.entry(&owner) else {
                continue;
            };
            let canonical = host.canonical_str(&qualified);
            let Some(variant) = canonical.rsplit("::").next() else {
                continue;
            };
            let count = variant_owners.len();
            let hidden = variant_owners
                .entry((source, owner.clone()))
                .or_insert_with(|| format!("\u{2}{count}").into())
                .clone();
            imports
                .entry((source, hidden.clone()))
                .or_insert_with(|| item::Binding {
                    name: crate::item::Name {
                        text: hidden.clone(),
                        span: Span::NONE,
                    },
                    qualified: owner.clone(),
                    target: item::Import::Type {
                        definition: definition.clone(),
                        owner: host.type_owner(&owner).cloned(),
                    },
                });
            host_variants_by_name.insert(
                (source, name),
                Box::from([hidden, ImmutableString::from(variant)]),
            );
        }
        let mut host_variants: BTreeMap<_, Box<[ImmutableString]>> = host_variants_by_name;

        let mut associated = AssociatedTypes::default();
        let mut impl_arguments = BTreeMap::<(ImmutableString, type_def::Id), ImplArguments>::new();
        let mut trait_instances = BTreeMap::<ImmutableString, Vec<Box<[type_ref::Type]>>>::new();
        for shell in program.items.iter() {
            let item::Kind::Impl(implementation) = &shell.kind else {
                continue;
            };
            let (
                Some(item::TraitImpl::User {
                    name,
                    arguments: trait_arguments,
                }),
                type_ref::Type::Path(owner),
            ) = (&implementation.trait_, &implementation.owner)
            else {
                continue;
            };
            // An impl for a builtin type is keyed by its witness.
            let definition = &match &owner.target {
                type_ref::Target::Script { definition, .. } => *definition,
                _ => match crate::resolve::builtin_owner(&implementation.owner) {
                    Some((code, _)) => type_def::Id::builtin(code),
                    None => continue,
                },
            };
            // `impl<T> Shape for Wrapper<T>`: the owner's type arguments bind
            // the impl's parameters.
            let parameters: Box<[Option<ImmutableString>]> = owner
                .arguments
                .iter()
                .filter_map(|argument| match argument {
                    type_ref::Argument::Type(type_ref::Type::Path(path))
                        if path.target == type_ref::Target::Parameter =>
                    {
                        Some(Some(path.name.clone()))
                    }
                    type_ref::Argument::Type(_) => Some(None),
                    type_ref::Argument::Lifetime(_) => None,
                })
                .collect();
            impls.insert((name.text.clone(), *definition));
            if !trait_arguments.is_empty() {
                impl_arguments
                    .entry((name.text.clone(), *definition))
                    .or_default()
                    .push((
                        implementation
                            .generics
                            .iter()
                            .map(|generic| generic.name.text.clone())
                            .collect(),
                        parameters.clone(),
                        trait_arguments.clone(),
                    ));
                if !trait_arguments.iter().any(mentions_parameter) {
                    let instances = trait_instances.entry(name.text.clone()).or_default();
                    let text = instance_text(trait_arguments);
                    if !instances
                        .iter()
                        .any(|instance| instance_text(instance) == text)
                    {
                        instances.push(trait_arguments.clone());
                    }
                }
            }
            // Keyed by trait: two traits may each give one type an
            // `Output`.
            for (assoc, ty) in &implementation.associated {
                let key = (*definition, name.text.clone(), assoc.text.clone());
                associated
                    .impls
                    .insert(key, (ty.clone(), parameters.clone()));
            }
        }
        // `impl<T: For> For for &T`, forwarding to `T`'s.
        for (_, declared) in traits.values() {
            for forward in declared.reference_forwards.iter() {
                associated
                    .reference_forwards
                    .insert((declared.name.text.clone(), forward.mutable));
            }
        }
        // `type Fore: Iterator;`, its item no equality names.
        for (_, declared) in traits.values() {
            for (index, name) in declared.associated.iter().enumerate() {
                let iterator = declared
                    .associated_bounds
                    .get(index)
                    .is_some_and(|(_, bounds)| {
                        bounds.iter().any(|bound| is_iterator_trait(&bound.text))
                    });
                let item = declared
                    .associated_equalities
                    .get(index)
                    .is_some_and(|equalities| {
                        equalities
                            .iter()
                            .any(|(bound, item, _)| is_iterator_trait(bound) && item.text == "Item")
                    });
                if iterator && !item {
                    associated
                        .iterators
                        .insert((declared.name.text.clone(), name.text.clone()));
                }
            }
        }
        // `type Fore: Iterator<Item = Self::For>;`.
        for (_, declared) in traits.values() {
            for (name, equalities) in declared
                .associated
                .iter()
                .zip(declared.associated_equalities.iter())
                .filter(|(_, equalities)| !equalities.is_empty())
            {
                associated.equalities.insert(
                    (declared.name.text.clone(), name.text.clone()),
                    equalities
                        .iter()
                        .map(|(bound, item, ty)| (bound.clone(), item.text.clone(), ty.clone()))
                        .collect(),
                );
            }
        }

        // The prelude's own `From` instances (`From<bool>` for `i64`), and
        // one `Into` instance per conversion target.
        let prelude = |name: &str| {
            traits
                .get(name)
                .is_some_and(|(_, declared)| declared.name.span == Span::NONE)
        };
        let builtin_path = |name: &str| {
            type_ref::Type::Path(type_ref::Path {
                name: name.into(),
                target: type_ref::Target::Builtin,
                arguments: Box::default(),
                span: Span::NONE,
            })
        };
        let mut add_instance = |trait_name: &str, instance: type_ref::Type| {
            let instances = trait_instances.entry(trait_name.into()).or_default();
            let text = instance_text(std::slice::from_ref(&instance));
            if !instances.iter().any(|known| instance_text(known) == text) {
                instances.push(Box::new([instance]));
            }
        };
        if prelude("From") {
            for source in ["bool", "char"] {
                add_instance("From", builtin_path(source));
            }
        }
        if prelude("Into") {
            for target in ["i64", "f64", "String"] {
                add_instance("Into", builtin_path(target));
            }
            for method in conversions.values().flatten() {
                if !method.owner_arguments().is_empty() {
                    continue;
                }
                let target = match method.builtin_owner() {
                    Some(owner) => owner.clone(),
                    None => type_ref::Type::Path(type_ref::Path {
                        name: method.owner_name().into(),
                        target: type_ref::Target::Script {
                            definition: method.owner(),
                            identity: method.owner_identity().clone(),
                        },
                        arguments: Box::default(),
                        span: Span::NONE,
                    }),
                };
                add_instance("Into", target);
            }
        }

        // An alias of a script type names it in expressions too:
        // `Alias { .. }` and `Alias::new()`.
        for ((source, name), alias) in &program.aliases {
            if let Some(target) = &alias.target
                && let type_ref::Type::Path(path) = &**target
                && let type_ref::Target::Script { definition, .. } = &path.target
                && path.arguments.is_empty()
                && let Some(nominal) = by_definition.get(definition)
            {
                by_name
                    .entry((*source, name.clone()))
                    .or_insert_with(|| Nominal::clone(nominal));
            }
        }
        // A derive's expansion names what its item's source does (#593).
        let mut aliases = program.aliases.clone();
        for (expansion, item) in program.source.expansions() {
            inherit_names(&mut by_name, item, expansion);
            inherit_names(&mut functions, item, expansion);
            inherit_names(&mut imports, item, expansion);
            inherit_names(&mut variants, item, expansion);
            inherit_names(&mut host_variants, item, expansion);
            inherit_names(&mut aliases, item, expansion);
        }
        #[cfg(feature = "rust-import")]
        let mirrored = std::sync::Arc::new(
            program
                .mirrored(&host)
                .map(|(identity, definition)| (identity, definition.id()))
                .collect(),
        );
        // Each type this program's lowering reads names a mirror's marked
        // type, as its native signatures do (#1484).
        #[cfg(feature = "rust-import")]
        super::ty::lower_mirrors(std::sync::Arc::clone(&mirrored));
        let tables = Tables {
            runtime,
            host,
            access: engine.access().clone(),
            test: engine.test(),
            features: program.features().clone(),
            imports,
            lexical: program.lexical.clone(),
            block_scopes,
            by_name,
            declared_in,
            variants,
            host_variants,
            by_definition,
            by_identity,
            qualified_functions: program.qualified_functions.clone(),
            qualified_owners: program.qualified_owners.clone(),
            qualified_pattern_owners: program.qualified_pattern_owners.clone(),
            qualified_rust: program.qualified_rust.clone(),
            trait_names: program.trait_names.clone(),
            entry_crate: program.entry_crate.clone(),
            #[cfg(feature = "rust-import")]
            mirrors: program.mirrors.keys().cloned().collect(),
            #[cfg(feature = "rust-import")]
            mirrored,
            crates: program.crates.clone(),
            aliases,
            namespaces: program
                .scripts
                .iter()
                .map(|script| (script.source, script.namespace.clone()))
                .collect(),
            methods,
            conversions,
            trait_methods,
            traits,
            impls,
            static_impls: program
                .items
                .iter()
                .filter_map(|shell| match &shell.kind {
                    item::Kind::Impl(implementation) if implementation.statics => {
                        match &implementation.owner {
                            type_ref::Type::Path(type_ref::Path {
                                target: type_ref::Target::Script { definition, .. },
                                ..
                            }) => Some(*definition),
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .collect(),
            impl_arguments,
            trait_instances,
            associated: Shared::new(associated),
            std_traits,
            index_mut: program
                .methods
                .iter()
                .filter(|definition| {
                    definition.name() == "index_mut"
                        && definition
                            .trait_name()
                            .is_some_and(|name| name.as_str() == "IndexMut")
                })
                .filter_map(|definition| {
                    let [parameter] = definition.parameters() else {
                        return None;
                    };
                    if parameter.pattern.is_some() {
                        return None;
                    }
                    let text = source_span(
                        program.source.text_at(definition.source()),
                        definition.body(),
                    );
                    Some((definition.owner(), (parameter.name.text.clone(), text)))
                })
                .collect(),
            functions,
            functions_by_key,
            sources: program.source.table().clone(),
            entry: program.source.entry(),
            instances: program.instances.clone(),
            macros: program.macros.clone(),
            macro_rules: program
                .macros
                .definitions()
                .map(|definition| {
                    let text = source_span(
                        program.source.text_at(definition.source()),
                        definition.span(),
                    );
                    let tokens = lex_rust(engine, &text);
                    (
                        (definition.source(), definition.span()),
                        crate::resolve::macro_rules::Rules::from_tokens(&tokens).map(Shared::new),
                    )
                })
                .collect(),
            script_impls: program
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, shell)| {
                    let item::Kind::Impl(implementation) = &shell.kind else {
                        return None;
                    };
                    let Some(item::TraitImpl::Native {
                        path, arguments, ..
                    }) = &implementation.trait_
                    else {
                        return None;
                    };
                    matches!(
                        &implementation.owner,
                        type_ref::Type::Path(type_ref::Path {
                            target: type_ref::Target::Script { .. },
                            ..
                        })
                    )
                    .then(|| ScriptTraitImpl {
                        index,
                        path: path.clone(),
                        generics: implementation
                            .generics
                            .iter()
                            .map(|generic| generic.name.text.clone())
                            .collect(),
                        owner: implementation.owner.clone(),
                        arguments: arguments.clone(),
                    })
                })
                .collect(),
        };
        Self {
            context: program.source.entry(),
            tables: Shared::new(tables),
            global_head: None,
            at: Position::NONE,
            #[cfg(feature = "rust-import")]
            _imported_owner: None,
            #[cfg(test)]
            _meter: Meter,
        }
    }

    pub(super) fn get(&self, name: &str) -> Option<&Nominal> {
        self.by_name
            .get(&(self.context, name.into()))
            .filter(|_| self.in_scope(name, self.at))
    }

    /// Whether `name` is in scope at `at` in this source as far as `use`s
    /// say: everywhere, unless only blocks' `use`s bind it, and then inside
    /// those blocks.
    pub(super) fn in_scope(&self, name: &str, at: Position) -> bool {
        at.is_none()
            || self
                .block_scopes
                .get(&(self.context, name.into()))
                .is_none_or(|blocks| {
                    blocks
                        .iter()
                        .any(|block| block.start() <= at && at <= block.end())
                })
    }

    /// The enum an imported variant's name at `at` stands for in this
    /// source.
    pub(super) fn imported_variant(&self, name: &str, at: Position) -> Option<&[ImmutableString]> {
        self.variants
            .get(&(self.context, name.into()))
            .filter(|_| self.in_scope(name, at))
            .map(AsRef::as_ref)
    }

    pub(super) fn imports_variants(&self) -> bool {
        [&self.variants, &self.host_variants]
            .into_iter()
            .any(|variants| {
                variants
                    .range((self.context, ImmutableString::new())..)
                    .next()
                    .is_some_and(|((source, _), _)| *source == self.context)
            })
    }

    /// The whole path of the crate enum variant a name at `at` stands for
    /// in this source (`use iced::Center;`).
    pub(super) fn imported_host_variant(
        &self,
        name: &str,
        at: Position,
    ) -> Option<&[ImmutableString]> {
        self.host_variants
            .get(&(self.context, name.into()))
            .filter(|_| self.in_scope(name, at))
            .map(AsRef::as_ref)
    }

    pub(super) fn alias(&self, name: &str) -> Option<&type_ref::Alias> {
        self.aliases.get(&(self.context, name.into()))
    }

    pub(super) fn qualified_owner(&self, span: Span) -> Option<&Nominal> {
        let identity = self.qualified_owners.get(&(self.context, span))?;
        self.by_identity.get(identity)
    }

    /// The app-crate spelling of a qualified path into a `.rs` module.
    pub(super) fn qualified_rust(&self, span: Span) -> Option<&[ImmutableString]> {
        self.qualified_rust
            .get(&(self.context, span))
            .map(AsRef::as_ref)
    }

    pub(super) fn qualified_pattern_owner(&self, span: Span) -> Option<&Nominal> {
        let identity = self.qualified_pattern_owners.get(&(self.context, span))?;
        self.by_identity.get(identity)
    }

    /// Whether the source being lowered declares the script type `id`.
    pub(super) fn declared_here(&self, id: type_def::Id) -> bool {
        self.declared_in.get(&id) == Some(&self.context)
    }

    pub(super) fn definition(&self, id: type_def::Id) -> &Nominal {
        &self.by_definition[&id]
    }

    /// What the runner holds the script type `id`'s values as where a
    /// crate's generic takes one: the mirror rawr's copy of a native root's
    /// crate declares for a marked type native code sees, whose values
    /// native code names (#1249), and else its proxy (D594).
    #[cfg(feature = "rust-import")]
    pub(super) fn holder(
        &self,
        id: type_def::Id,
    ) -> rawr_import::classify::representability::convert::Holder {
        use rawr_import::classify::representability::convert::Holder;
        let key = self.definition(id).definition().key();
        let path = std::iter::once(ImmutableString::from(key.package()))
            .chain(key.modules().iter().cloned())
            .chain([ImmutableString::from(key.name())])
            .collect::<Box<[_]>>();
        if self.mirrors.contains(&path) {
            Holder::Mirror
        } else {
            Holder::Proxy
        }
    }

    /// The marked type `definition` mirrors in rawr's copy of a native
    /// root's crate, which marked code holds as itself.
    #[cfg(feature = "rust-import")]
    pub(super) fn mirrored(
        &self,
        definition: &crate::host::type_def::Definition,
    ) -> Option<type_def::Id> {
        self.mirrored
            .get(definition.identity().imported()?)
            .copied()
    }

    /// The path the runner names the script type `id`'s proxy by, its
    /// crate's name first, as the script names it (`counter::State`,
    /// `counter::app::State`) (D594). A universe runner names it by its
    /// index in the program's definition order (`counter::__RawrScript1`,
    /// `model::crossing_name`), so it serves every program whose native
    /// calls are the same.
    #[cfg(feature = "rust-import")]
    pub(super) fn script_path(&self, id: type_def::Id) -> Box<str> {
        let key = self.definition(id).definition().key();
        let below = key
            .modules()
            .iter()
            .map(ImmutableString::as_str)
            .chain([key.name()])
            .collect::<Vec<_>>()
            .join("::");
        format!(
            "{}::{}",
            key.package().replace('-', "_"),
            self.runtime.crossing_path(&below)
        )
        .into_boxed_str()
    }

    /// The inherent function `name` of `owner`, or else the one trait
    /// function of that name its trait impls provide, as Rust looks them up.
    /// An inherent function this source can't see is passed over, as
    /// rustc's probe passes over a private candidate.
    pub(super) fn method(&self, owner: type_def::Id, name: &str) -> Option<&method::Definition> {
        if let Some(inherent) = self
            .methods
            .get(&(owner, name.into()))
            .filter(|method| self.sees(method))
        {
            return Some(inherent);
        }
        match self.trait_methods.get(&(owner, name.into()))?.as_slice() {
            [single] => Some(single),
            _ => None,
        }
    }

    /// The inherent function `name` of `owner` that is private to a module
    /// this source is outside of: rustc's E0624 where nothing else of that
    /// name is found.
    pub(super) fn private_method(
        &self,
        owner: type_def::Id,
        name: &str,
    ) -> Option<&method::Definition> {
        self.methods
            .get(&(owner, name.into()))
            .filter(|method| !self.sees(method))
    }

    /// Whether this source sees `method`: a public one, a trait impl's
    /// (which is as visible as its trait), or one private to this source's
    /// module or one of its ancestors.
    pub(super) fn sees(&self, method: &method::Definition) -> bool {
        method.is_public()
            || method.trait_name().is_some()
            || method.source() == self.context
            || match (
                self.namespaces.get(&self.context),
                self.namespaces.get(&method.source()),
            ) {
                (Some(here), Some(module)) => here.sees_private(module),
                _ => false,
            }
    }

    /// Whether `owner` implements the crate trait `trait_name`.
    pub(super) fn implements(&self, trait_name: &str, owner: type_def::Id) -> bool {
        self.impls.contains(&(trait_name.into(), owner))
    }

    /// The hand-written function `name` of the std trait `trait_name` for
    /// `owner`: `impl fmt::Display for Owner`'s `fmt`, for example.
    pub(super) fn std_impl(
        &self,
        owner: type_def::Id,
        trait_name: &str,
        name: &str,
    ) -> Option<&method::Definition> {
        if !self.std_traits.contains(trait_name) {
            return None;
        }
        self.trait_methods
            .get(&(owner, name.into()))?
            .iter()
            .find(|definition| {
                definition
                    .trait_name()
                    .is_some_and(|implemented| implemented == trait_name)
            })
    }

    /// Whether the model builds `ty`'s `Default` once, when the program is
    /// compiled. A type with its own `impl Default` and an imported Rust
    /// type run code for it, and so does a struct deriving `Default` with
    /// such a field (`application::default_runs_code` reads the same).
    pub(super) fn default_folds(&self, ty: &Ty, visiting: &mut Vec<type_def::Id>) -> bool {
        match ty {
            Ty::Boxed(inner) | Ty::Reverse(inner) => self.default_folds(inner, visiting),
            // A cell is made where its default is used, so copies share
            // no cell, and so is a pointer, so each is an allocation.
            Ty::Cell(..) | Ty::Counted(..) | Ty::Weak(..) => false,
            Ty::Array {
                element,
                kind: sequence::Kind::Array(_),
            } => self.default_folds(element, visiting),
            Ty::Tuple(members) => members
                .iter()
                .all(|member| self.default_folds(member, visiting)),
            Ty::Host { proof, .. } => proof.definition.native_identity().is_some(),
            Ty::Script(id, _) => {
                if visiting.contains(id) {
                    return true;
                }
                if self.std_impl(*id, "Default", "default").is_some() {
                    return false;
                }
                let Nominal::Struct { item, .. } = self.definition(*id) else {
                    return true;
                };
                visiting.push(*id);
                let folds = item
                    .fields
                    .iter()
                    .all(|field| self.default_folds(&Ty::from_resolved(&field.ty), visiting));
                visiting.pop();
                folds
            }
            _ => true,
        }
    }

    /// Whether a value of `ty` holds a `Cell` or `RefCell` of its own, not
    /// behind an `Rc` or `Arc`. Its runtime copy would share the cell,
    /// while Rust's `clone` makes a new one.
    pub(super) fn holds_cell(&self, ty: &Ty, visiting: &mut Vec<type_def::Id>) -> bool {
        match ty {
            Ty::Cell(..) => true,
            Ty::Boxed(inner)
            | Ty::Reverse(inner)
            | Ty::Cow(inner)
            | Ty::Option(inner)
            | Ty::Array { element: inner, .. } => self.holds_cell(inner, visiting),
            Ty::Result(ok, err) => self.holds_cell(ok, visiting) || self.holds_cell(err, visiting),
            Ty::Tuple(members) | Ty::Keyed(_, members) => members
                .iter()
                .any(|member| self.holds_cell(member, visiting)),
            Ty::Script(id, arguments) => {
                if visiting.contains(id) || self.std_impl(*id, "Clone", "clone").is_some() {
                    return false;
                }
                if arguments
                    .iter()
                    .any(|argument| self.holds_cell(argument, visiting))
                {
                    return true;
                }
                visiting.push(*id);
                let holds = match self.definition(*id) {
                    Nominal::Struct { item, .. } => item
                        .fields
                        .iter()
                        .any(|field| self.holds_cell(&Ty::from_resolved(&field.ty), visiting)),
                    Nominal::Enum { item, .. } => item.variants.iter().any(|variant| {
                        variant
                            .payload
                            .iter()
                            .any(|ty| self.holds_cell(&Ty::from_resolved(ty), visiting))
                    }),
                };
                visiting.pop();
                holds
            }
            _ => false,
        }
    }

    pub(super) fn function(&self, name: &str) -> Option<&ScriptFunction> {
        self.functions
            .get(&(self.context, name.into()))
            .filter(|_| self.in_scope(name, self.at))
    }

    pub(super) fn qualified_function(&self, span: Span) -> Option<&ScriptFunction> {
        let key = self.qualified_functions.get(&(self.context, span))?;
        self.functions_by_key.get(key)
    }

    /// The path the crate trait this source names `name` is known by, when
    /// another module's trait has that name too, as `sidebar::Shape` of
    /// `Shape` in `mod sidebar`.
    pub(super) fn trait_name(&self, name: &str) -> Option<&ImmutableString> {
        self.trait_names.get(&(self.context, name.into()))
    }

    pub(super) fn import(&self, name: &str) -> Option<&crate::resolve::item::Binding> {
        if self
            .global_head
            .as_ref()
            .is_some_and(|head| head.as_str() == name)
        {
            return None;
        }
        self.imports
            .get(&(self.context, name.into()))
            .filter(|_| self.in_scope(name, self.at))
    }

    pub(super) fn for_source(&self, source: crate::program::source::Id) -> Self {
        let mut models = self.clone();
        models.context = source;
        models
    }

    pub(super) fn host_items(&self, path: &str) -> Option<&[crate::host::item::Definition]> {
        self.host.overloads(path)
    }

    pub(super) fn item_owner(&self, path: &str) -> Option<&crate::host::facade::Dependency> {
        self.host.item_owner(path)
    }

    pub(super) fn support_id(&self) -> u64 {
        let proof: Vec<_> = self
            .by_identity
            .iter()
            .map(|(identity, model)| (identity, model.definition().schema()))
            .collect();
        crate::program::schema::symbol_id(&proof)
    }
}

impl Nominal {
    pub(super) fn definition(&self) -> &type_def::Definition {
        match self {
            Self::Struct { definition, .. } | Self::Enum { definition, .. } => definition,
        }
    }

    pub(super) fn name(&self) -> &str {
        self.definition().name()
    }

    pub(super) fn identity(&self) -> type_def::Identity {
        type_def::Identity::Script(self.definition().key().clone())
    }

    pub(super) fn ty(&self) -> Ty {
        Ty::Script(self.definition().id(), Box::default())
    }

    /// The type's generic parameters.
    pub(super) fn generics(&self) -> &[item::Generic] {
        match self {
            Self::Struct { item, .. } => &item.generics,
            Self::Enum { item, .. } => &item.generics,
        }
    }

    pub(super) fn capabilities(&self) -> capability::Set {
        self.definition().capabilities()
    }
}
