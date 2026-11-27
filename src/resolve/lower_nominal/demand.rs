//! Demand reads at the runner's fixed arguments (decision 3, #2698). An
//! opened module's generic impl or function whose parameters the runner's
//! fixed arguments settle runs from source at those types once a program
//! reaches it, as iced's `Container` does at `crate::Message`, iced's
//! `Theme` and its renderer. The runner must already hold each native call
//! such a body makes, or a save that first builds a container rebuilds
//! it. So each such body is read at those types for its native uses alone:
//! the impls of a crate trait a settled use fits (`exact.rs::seed_demand`),
//! an inherent impl at its type's declared defaults, and each template one
//! of those reads calls at settled types, in turn. A parameter the fixed
//! arguments leave open, as the content `W` of `Container`, stays the
//! body's own, read through its bound, and a statement that can't be
//! settled is dropped (`statement.rs::tolerant`). Each defined item at one
//! set of types is read once (`Done`). Nothing a demand read makes is a
//! program item, instance or check: its settled native uses join the
//! runner's (`Instances::ahead`), and its exact native instances are bound
//! as the program's check binds them.

use super::imported::{
    instance_bounds, into_conversions, named_type, substitute_parameters, take_impl_uses,
    take_instance_calls,
};
use super::models::Models;
use super::ty::Ty;
use super::{body_source, source_span, ImportedContext, ImportedUse, Parser};
use crate::resolve::{item, type_ref, Program};
use crate::{method, Engine, ImmutableString, Span};
use std::collections::{BTreeMap, BTreeSet};

/// A script impl of a crate trait that a settled use of the trait fits,
/// by its item's index, with the impl's parameters the use fixes.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub(super) struct Seed {
    pub(super) index: usize,
    pub(super) fixed: Vec<(ImmutableString, Ty)>,
}

/// The exact native instances a pass of demand reads bound, each by its
/// trait, the member a call through a bound names (none for the whole
/// trait), and its types' names (`Parser::demand_bound`); and the types
/// whose inherent members were bound (`exact.rs::bind_inherent_members`),
/// each by its definition and its name, with no member.
pub(super) type Bound = BTreeSet<([u8; 32], Option<ImmutableString>, Box<[ImmutableString]>)>;

/// The templates read for demand, each once.
pub(super) type Done = crate::resolve::Demanded;

/// The native uses the demand reads `seeds` and the opened inherent impls
/// at their types' defaults reach, each template read once at one set of
/// types, whatever `done` already holds left out.
pub(super) fn read(
    engine: &Engine,
    program: &Program,
    models: &Models,
    allocation: &mut dyn crate::host::ImportAllocation,
    seeds: &[Seed],
    done: &mut Done,
) -> Vec<ImportedUse> {
    let mut pending = Vec::new();
    for seed in seeds.iter().chain(defaulted(program).iter()) {
        impl_templates(program, seed.index, &seed.fixed, &mut pending);
    }
    // Only an opened module's impls are seeds; the templates their bodies
    // call are read whoever declares them.
    pending.retain(|(template, _)| opened(template));
    let mut demanded = Vec::new();
    let mut bound = Bound::new();
    while let Some((template, fixed)) = pending.pop() {
        let names = fixed
            .iter()
            .map(|(name, ty)| {
                (
                    name.clone(),
                    ImmutableString::from(ty.name(models).to_string()),
                )
            })
            .collect::<Box<[_]>>();
        if !done.insert((template.clone(), names)) {
            continue;
        }
        let Some(read) = read_one(
            engine,
            program,
            models,
            allocation,
            (&template, &fixed[..]),
            &mut bound,
        ) else {
            continue;
        };
        demanded.extend(read.uses);
        for (called, types, _) in read.calls {
            let Some(generics) = template_generics(program, &called) else {
                continue;
            };
            let fixed = generics
                .iter()
                .zip(&types)
                .filter(|(_, ty)| settled(ty, models))
                .map(|(name, ty)| (name.clone(), ty.clone()))
                .collect::<Vec<_>>();
            if !fixed.is_empty() {
                pending.push((called, fixed));
            }
        }
        for (index, types, _) in read.impl_uses {
            let fixed = types
                .into_iter()
                .filter(|(_, ty)| settled(ty, models))
                .collect::<Vec<_>>();
            if !fixed.is_empty() {
                impl_templates(program, index, &fixed, &mut pending);
            }
        }
    }
    demanded
}

/// What one demand read found: its settled native uses, and the
/// templates and script impls it calls, at the types it gives them.
struct Read {
    uses: Vec<ImportedUse>,
    calls: Vec<(crate::item::Key, Vec<Ty>, Span)>,
    impl_uses: Vec<super::imported::ImplUse>,
}

/// Each template among the functions of the impl item `index`, at `fixed`.
fn impl_templates(
    program: &Program,
    index: usize,
    fixed: &[(ImmutableString, Ty)],
    pending: &mut Vec<(crate::item::Key, Vec<(ImmutableString, Ty)>)>,
) {
    let Some(item::Kind::Impl(implementation)) = program.items.get(index).map(|shell| &shell.kind)
    else {
        return;
    };
    for function in implementation.functions.iter() {
        if let item::Lowering::NominalMethod(id) = function.body.lowering
            && let Some(definition) = program.methods.get(id.index())
            && program.instances.templates.contains_key(definition.key())
        {
            pending.push((definition.key().clone(), fixed.to_vec()));
        }
    }
}

/// Each inherent impl of an opened module's generic type whose header
/// writes a parameter where the type declares a default, with that
/// parameter at the default, as `Theme` of iced's `impl<'a, W, Theme>
/// Container<'a, W, Theme>` is `crate::Theme`: the runner's fixed argument
/// there.
fn defaulted(program: &Program) -> Vec<Seed> {
    let mut seeds = Vec::new();
    for (index, shell) in program.items.iter().enumerate() {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        // A generic type's own impl, or the trait an opened module holds a
        // concrete type's generic inherent methods in.
        let inherent = match &implementation.trait_ {
            None => true,
            Some(item::TraitImpl::User { name, .. }) => {
                crate::package::crate_source::is_inherent(&name.text)
            }
            Some(item::TraitImpl::Native { .. }) => false,
        };
        if !inherent {
            continue;
        }
        let type_ref::Type::Path(type_ref::Path {
            target: type_ref::Target::Script { definition, .. },
            arguments,
            ..
        }) = &implementation.owner
        else {
            continue;
        };
        let Some(declared) = program.items.iter().find_map(|shell| match &shell.kind {
            item::Kind::Struct(declared) if declared.definition == *definition => {
                Some(&declared.generics)
            }
            _ => None,
        }) else {
            continue;
        };
        let written = arguments.iter().filter_map(|argument| match argument {
            type_ref::Argument::Type(ty) => Some(ty),
            type_ref::Argument::Lifetime(_) => None,
        });
        let fixed = declared
            .iter()
            .zip(written)
            .filter_map(|(generic, written)| {
                let default = generic.default.as_ref()?;
                let type_ref::Type::Path(type_ref::Path {
                    name,
                    target: type_ref::Target::Parameter,
                    arguments,
                    ..
                }) = written
                else {
                    return None;
                };
                (arguments.is_empty() && !super::mentions_parameter(default))
                    .then(|| (name.clone(), Ty::from_resolved(default)))
            })
            .collect::<Vec<_>>();
        if !fixed.is_empty() {
            seeds.push(Seed { index, fixed });
        }
    }
    seeds
}

/// Whether `key` is an item of an opened module's program module.
fn opened(key: &crate::item::Key) -> bool {
    key.modules()
        .first()
        .is_some_and(|module| crate::package::crate_source::is_name(module))
}

/// The names of the generic parameters of the template `key`, in order.
fn template_generics(program: &Program, key: &crate::item::Key) -> Option<Vec<ImmutableString>> {
    if !program.instances.templates.contains_key(key) {
        return None;
    }
    let names = |generics: &[item::Generic]| {
        generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .collect()
    };
    if let Some(definition) = method_of(program, key) {
        return Some(names(definition.generics()));
    }
    program.items.iter().find_map(|shell| match &shell.kind {
        item::Kind::Function(definition) if shell.key.as_ref() == Some(key) => {
            Some(names(&definition.generics))
        }
        _ => None,
    })
}

fn method_of<'p>(program: &'p Program, key: &crate::item::Key) -> Option<&'p method::Definition> {
    program
        .methods
        .iter()
        .chain(program.builtin_methods.iter())
        .find(|definition| definition.key() == key)
}

/// Whether `ty` is settled and a signature can name it.
fn settled(ty: &Ty, models: &Models) -> bool {
    !super::ty::contains_inference(ty)
        && !super::ty::reaches_param(ty)
        && named_type(ty, models, Span::NONE).is_some()
}

/// A demand read's param-env: the crate-trait bounds `generics` declare,
/// each fixed parameter's at its type and each other's on the parameter
/// itself, both at the types `substituted` gives, as `W: Widget<Message,
/// Theme, Renderer>` of `Container`'s impl fixes the trait's arguments of
/// a call on its content to the runner's.
fn param_env(
    generics: &[item::Generic],
    substituted: &[(ImmutableString, type_ref::Type)],
) -> Box<[(type_ref::Type, type_ref::Type)]> {
    let fixed = instance_bounds(generics, substituted);
    let kept = kept_generics(generics, substituted);
    let open = kept.iter().flat_map(|generic| {
        let parameter = type_ref::Type::Path(type_ref::Path {
            name: generic.name.text.clone(),
            target: type_ref::Target::Parameter,
            arguments: Box::default(),
            span: Span::NONE,
        });
        generic
            .crate_bounds
            .iter()
            .map(move |bound| (parameter.clone(), bound.clone()))
    });
    fixed.iter().cloned().chain(open).collect()
}

/// `generics` past those `substituted` fixes, their bounds at its types.
fn kept_generics(
    generics: &[item::Generic],
    substituted: &[(ImmutableString, type_ref::Type)],
) -> Box<[item::Generic]> {
    let at = |ty: &type_ref::Type| substitute_parameters(ty, substituted);
    generics
        .iter()
        .filter(|generic| {
            !substituted
                .iter()
                .any(|(name, _)| *name == generic.name.text)
        })
        .map(|generic| {
            let mut kept = generic.clone();
            kept.crate_bounds = kept.crate_bounds.iter().map(at).collect();
            kept.projections = kept
                .projections
                .iter()
                .map(|(bound, name, ty)| (bound.clone(), name.clone(), at(ty)))
                .collect();
            for (_, arguments) in kept.trait_arguments.iter_mut() {
                *arguments = arguments.iter().map(at).collect();
            }
            kept.default = kept.default.as_ref().map(at);
            kept.fixed = kept.fixed.as_ref().map(at);
            kept
        })
        .collect()
}

/// The demand read of the template `template` with the parameters
/// `fixed` gives at their types.
fn read_one(
    engine: &Engine,
    program: &Program,
    models: &Models,
    allocation: &mut dyn crate::host::ImportAllocation,
    (template, fixed): (&crate::item::Key, &[(ImmutableString, Ty)]),
    bound: &mut Bound,
) -> Option<Read> {
    let substituted = fixed
        .iter()
        .map(|(name, ty)| Some((name.clone(), named_type(ty, models, Span::NONE)?)))
        .collect::<Option<Box<[_]>>>()?;
    let key = template.renamed(format!("{}#demand", template.name()).into());
    let mut uses = Vec::new();
    let mut dependencies = BTreeSet::new();
    let mut reads = Vec::new();
    let mut failure = None;
    // This round's generic reads, which the round's models predate.
    let selections = program.instances.selections.get(template).cloned();
    let setup =
        |parser: &mut Parser<'_>, generics: &[item::Generic], parameters: &[item::Parameter]| {
            parser.symbolic_only = true;
            parser.collecting_instances = true;
            parser.selecting = true;
            parser.demand = Some(param_env(generics, &substituted));
            for (name, ty) in fixed {
                parser.trait_parameters.insert(name.clone(), ty.clone());
            }
            if let Some(selections) = &selections {
                parser.spliced_traits.extend(selections.iter().cloned());
            }
            parser.instance_conversions = into_conversions(parameters, generics, &substituted)
                .iter()
                .filter_map(|(name, target)| {
                    let parameter = parameters
                        .iter()
                        .find(|parameter| parameter.name.text == *name)?;
                    Some((parameter.name.span, Ty::from_resolved(target)))
                })
                .collect::<BTreeMap<_, _>>();
        };
    let finish = |parser: &mut Parser<'_>, bound: &mut Bound| {
        let read = Read {
            uses: parser.demanded_uses(),
            calls: take_instance_calls(parser),
            impl_uses: take_impl_uses(parser),
        };
        *bound = std::mem::take(&mut parser.demand_bound);
        read
    };
    if let Some(definition) = method_of(program, template) {
        let instance = definition.demand_instance(
            key,
            format!("{}#demand", definition.name()).into(),
            |ty| substitute_parameters(ty, &substituted),
            kept_generics(definition.generics(), &substituted),
        );
        let text = source_span(
            definition
                .body_text()
                .unwrap_or_else(|| program.source.text_at(definition.source())),
            definition.body(),
        );
        let context = ImportedContext {
            native: None,
            allocation,
            failure: &mut failure,
            source: definition.source(),
            uses: &mut uses,
            dependencies: &mut dependencies,
            reads: &mut reads,
        };
        let mut parser = Parser::for_method(
            engine,
            &text,
            models.for_source(definition.source()),
            &instance,
            0,
            0,
            Some(context),
        )
        .ok()?;
        setup(&mut parser, definition.generics(), definition.parameters());
        parser.demand_bound = std::mem::take(bound);
        let _ = parser
            .expect_output(Some(instance.output()))
            .and_then(|()| parser.parse_method_body(&instance));
        return Some(finish(&mut parser, bound));
    }
    let (shell, definition) = program.items.iter().find_map(|shell| match &shell.kind {
        item::Kind::Function(definition) if shell.key.as_ref() == Some(template) => {
            Some((shell, definition))
        }
        _ => None,
    })?;
    let mut instance = definition.clone();
    instance.generics = kept_generics(&definition.generics, &substituted);
    if instance.lifetimes.written_parameters.is_none() {
        instance.lifetimes.written_parameters = Some(
            definition
                .parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect(),
        );
    }
    for parameter in instance.parameters.iter_mut() {
        parameter.ty = substitute_parameters(&parameter.ty, &substituted);
    }
    instance.output = instance
        .output
        .as_ref()
        .map(|output| substitute_parameters(output, &substituted));
    if instance.lifetimes.written_output.is_none() {
        instance.lifetimes.written_output = definition.output.clone().map(Box::new);
    }
    let text = body_source(
        definition
            .body
            .text
            .as_deref()
            .unwrap_or_else(|| program.source.text_at(shell.source)),
        &definition.body,
    );
    let context = ImportedContext {
        native: None,
        allocation,
        failure: &mut failure,
        source: shell.source,
        uses: &mut uses,
        dependencies: &mut dependencies,
        reads: &mut reads,
    };
    let mut parser = Parser::for_function(
        engine,
        &text,
        models.for_source(shell.source),
        &instance,
        key,
        0,
        0,
        Some(context),
    );
    setup(&mut parser, &definition.generics, &definition.parameters);
    parser.demand_bound = std::mem::take(bound);
    let _ = parser
        .expect_output(instance.output.as_ref())
        .and_then(|()| parser.parse_function_body(&instance));
    Some(finish(&mut parser, bound))
}
