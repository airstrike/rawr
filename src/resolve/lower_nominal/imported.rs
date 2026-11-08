//! Crate instantiation: revealing the program's uses of imported crates,
//! and instantiating the generic crate items and impls they reach.

#[cfg(feature = "rust-import")]
use super::models::Models;
use super::ty::Ty;
#[cfg(feature = "rust-import")]
use super::ty::{Float, Int};
#[cfg(feature = "rust-import")]
use super::{
    body_source, contains_open_literal, holds_value, impl_surface_type, lifetime_text,
    mentions_parameter, sequence, source_span, ImportedContext, ImportedUse, Parser,
};
use crate::resolve::type_ref;
#[cfg(feature = "rust-import")]
use crate::resolve::{item, Error, LoweringFailure, Program};
use crate::ImmutableString;
#[cfg(feature = "rust-import")]
use crate::{method, type_def, Engine, Shared, Span};
#[cfg(feature = "rust-import")]
use std::collections::{BTreeMap, BTreeSet};

/// Each `impl Trait<A> for Owner` of a generic trait: the impl's own
/// parameters and the trait's arguments.
pub(super) type ImplArguments = Vec<(Box<[ImmutableString]>, Box<[type_ref::Type]>)>;

#[cfg(feature = "rust-import")]
pub(in crate::resolve) fn imported<A: crate::host::ImportAllocation>(
    engine: &Engine,
    program: &Program,
    host: &Shared<crate::host::Registry>,
    allocation: &mut A,
) -> Result<ImportedOutput, crate::resolve::ImportedFailure> {
    let runtime = crate::program::model::Model::new(crate::program::GenerationId::issue(), program);
    let models = Models::new(program, runtime, Shared::clone(host), engine);
    let mut uses = Vec::new();
    let mut dependencies = BTreeSet::new();
    let mut allocation_failure = None;
    let mut offset = 0;
    // A body that misses an item Rawr doesn't know doesn't stop the pass:
    // the next bodies' misses are asked for with it (#662).
    let mut missed = None;

    for shell in program.items.iter() {
        let item::Kind::Function(definition) = &shell.kind else {
            continue;
        };
        // A template compiled per instantiation is lowered as its
        // instances only (D629).
        if shell
            .key
            .as_ref()
            .is_some_and(|key| program.instances.templates.contains_key(key))
        {
            continue;
        }
        let body_source = body_source(
            definition
                .body
                .text
                .as_deref()
                .unwrap_or_else(|| program.source.text_at(shell.source)),
            &definition.body,
        );
        let mut lower = |recover: bool, failure: &mut Option<rawr_import::surface::Error>| {
            let mut parser = Parser::for_function(
                engine,
                &body_source,
                models.for_source(shell.source),
                definition,
                shell
                    .key
                    .clone()
                    .expect("resolved functions retain stable item keys"),
                offset,
                0,
                Some(ImportedContext {
                    native: None,
                    allocation: &mut *allocation,
                    failure,
                    source: shell.source,
                    uses: &mut uses,
                    dependencies: &mut dependencies,
                }),
            );
            parser.symbolic_only = true;
            parser.recover_misses = recover;
            let lowered = parser.parse_function_body(definition);
            (lowered, std::mem::take(&mut parser.recovered))
        };
        let first = lower(false, &mut allocation_failure);
        let (lowered, more) = again(first, || lower(true, &mut None));
        let Some(lowered) = body(
            lowered,
            more,
            &mut allocation_failure,
            shell.source,
            &mut missed,
        )?
        else {
            continue;
        };
        for operation in &lowered.ops {
            dependencies.extend(operation.dependencies());
        }
        offset = uses.len();
    }

    // An impl for a crate's or a builtin type is checked as the crate's own
    // are, so the native items its body uses are demanded too.
    for definition in program.methods.iter().chain(&program.builtin_methods) {
        // A method compiled per instantiation is lowered as its instances
        // only (D629).
        if program.instances.templates.contains_key(definition.key()) {
            continue;
        }
        let body_source = source_span(
            definition
                .body_text()
                .unwrap_or_else(|| program.source.text_at(definition.source())),
            definition.body(),
        );
        let mut lower = |recover: bool, failure: &mut Option<rawr_import::surface::Error>| {
            let mut parser = match Parser::for_method(
                engine,
                &body_source,
                models.for_source(definition.source()),
                definition,
                offset,
                0,
                Some(ImportedContext {
                    native: None,
                    allocation: &mut *allocation,
                    failure,
                    source: definition.source(),
                    uses: &mut uses,
                    dependencies: &mut dependencies,
                }),
            ) {
                Ok(parser) => parser,
                Err(error) => return (Err(error), Vec::new()),
            };
            parser.symbolic_only = true;
            parser.recover_misses = recover;
            let lowered = parser.parse_method_body(definition);
            (lowered, std::mem::take(&mut parser.recovered))
        };
        let first = lower(false, &mut allocation_failure);
        let (lowered, more) = again(first, || lower(true, &mut None));
        let Some(lowered) = body(
            lowered,
            more,
            &mut allocation_failure,
            definition.source(),
            &mut missed,
        )?
        else {
            continue;
        };
        for operation in &lowered.ops {
            dependencies.extend(operation.dependencies());
        }
        offset = uses.len();
    }

    let script = program
        .scripts
        .iter()
        .find(|script| script.source == program.source.entry())
        .expect("resolved package programs retain their entry script");
    let mut lower = |recover: bool, failure: &mut Option<rawr_import::surface::Error>| {
        let mut parser = Parser::new_with_offset(
            engine,
            &script.text,
            models.for_source(script.source),
            offset,
            0,
            Some(ImportedContext {
                native: None,
                allocation: &mut *allocation,
                failure,
                source: script.source,
                uses: &mut uses,
                dependencies: &mut dependencies,
            }),
        );
        parser.symbolic_only = true;
        parser.recover_misses = recover;
        let lowered = parser.parse();
        (lowered, std::mem::take(&mut parser.recovered))
    };
    let first = lower(false, &mut allocation_failure);
    let (lowered, more) = again(first, || lower(true, &mut None));
    let lowered = body(
        lowered,
        more,
        &mut allocation_failure,
        script.source,
        &mut missed,
    )?;
    if let Some(missed) = missed {
        return Err(crate::resolve::ImportedFailure::Lowering(missed));
    }
    let lowered = lowered.expect("a failed script body is missed");
    for operation in &lowered.ops {
        dependencies.extend(operation.dependencies());
    }
    uses.sort_unstable();
    uses.dedup();
    let impls = native_impls(program, &models)?.into_boxed_slice();
    Ok(ImportedOutput {
        uses: uses.into_boxed_slice(),
        impls,
    })
}

/// rustc's opaque return types of imported traits (`fn app() -> impl
/// IntoElement`): the hidden type is the body's, so each such function's
/// body is checked once against an inference variable in the `impl
/// Trait`'s place, and the signature names the type it binds from then on.
/// Callers, the runner and the native side then see one concrete type, as
/// rustc's codegen does; that callers may use more than the trait's methods
/// is a leniency `cargo check` reports. A body that calls another such
/// function waits for that one's reveal, round by round.
#[cfg(feature = "rust-import")]
pub(crate) fn reveal<A: crate::host::ImportAllocation>(
    engine: &Engine,
    program: &mut Program,
    host: &Shared<crate::host::Registry>,
    allocation: &mut A,
) -> Result<(), crate::resolve::ImportedFailure> {
    instance_templates(program);
    let instancing = body_templates(engine, program, host, allocation)?;
    #[derive(Clone, Copy)]
    enum Pending {
        Function(usize),
        Method(usize),
    }
    let opaque = |output: Option<&type_ref::Type>| output.is_some_and(type_ref::Type::has_opaque);
    let mut pending = program
        .items
        .iter()
        .enumerate()
        .filter(|(_, shell)| {
            matches!(&shell.kind, item::Kind::Function(definition)
                if opaque(definition.output.as_ref()))
                && !shell
                    .key
                    .as_ref()
                    .is_some_and(|key| program.instances.templates.contains_key(key))
        })
        .map(|(index, _)| Pending::Function(index))
        .chain(
            program
                .methods
                .iter()
                .enumerate()
                .filter(|(_, definition)| {
                    opaque(Some(definition.output()))
                        && !program.instances.templates.contains_key(definition.key())
                })
                .map(|(index, _)| Pending::Method(index)),
        )
        .collect::<Vec<_>>();
    // Generic functions compiled per instantiation (D629) take part round
    // by round: a call's types may come from a revealed output, and an
    // instance's own output may be opaque.
    let mut instance_rounds = 0;
    loop {
        let added = if instancing && instance_rounds < MAX_INSTANCE_ROUNDS {
            instance_rounds += 1;
            instance_round(engine, program, host, allocation)?
        } else {
            Vec::new()
        };
        // An instance comes before its callers, so a round that reveals
        // nothing reports the instance's own failure first.
        for &added in added.iter().rev() {
            match added {
                Added::Function(index) => {
                    if let item::Kind::Function(definition) = &program.items[index].kind
                        && opaque(definition.output.as_ref())
                    {
                        pending.insert(0, Pending::Function(index));
                    }
                }
                Added::Method(index) => {
                    if opaque(Some(program.methods[index].output())) {
                        pending.insert(0, Pending::Method(index));
                    }
                }
            }
        }
        if pending.is_empty() {
            if added.is_empty() {
                break;
            }
            continue;
        }
        let runtime =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), program);
        let models = Models::new(program, runtime, Shared::clone(host), engine);
        let mut revealed = Vec::new();
        let mut waiting = Vec::new();
        let mut first_failure = None;
        let mut first_unknown = None;
        for &next in &pending {
            let mut uses = Vec::new();
            let mut dependencies = BTreeSet::new();
            let mut allocation_failure = None;
            let recovered;
            let (source, result) = match next {
                Pending::Function(index) => {
                    let shell = &program.items[index];
                    let item::Kind::Function(definition) = &shell.kind else {
                        unreachable!("pending functions are function items");
                    };
                    let body_source = body_source(
                        definition
                            .body
                            .text
                            .as_deref()
                            .unwrap_or_else(|| program.source.text_at(shell.source)),
                        &definition.body,
                    );
                    let output = definition
                        .output
                        .as_ref()
                        .expect("an opaque function declares its output");
                    let mut lower =
                        |recover: bool, failure: &mut Option<rawr_import::surface::Error>| {
                            let mut parser = Parser::for_function(
                                engine,
                                &body_source,
                                models.for_source(shell.source),
                                definition,
                                shell
                                    .key
                                    .clone()
                                    .expect("resolved functions retain stable item keys"),
                                0,
                                0,
                                Some(ImportedContext {
                                    native: None,
                                    allocation: &mut *allocation,
                                    failure,
                                    source: shell.source,
                                    uses: &mut uses,
                                    dependencies: &mut dependencies,
                                }),
                            );
                            parser.symbolic_only = true;
                            parser.recover_misses = recover;
                            let result = parser.opaque_expected(output).and_then(|expected| {
                                parser.return_ty = Some(expected.clone());
                                parser.revealing = Some(expected.clone());
                                parser.parse_function_body(definition)?;
                                parser.fix_opaque(output, &expected)?;
                                let hidden = parser.inference.resolve(&expected);
                                named_output(output, &hidden, &parser.models)
                            });
                            (result, std::mem::take(&mut parser.recovered))
                        };
                    let first = lower(false, &mut allocation_failure);
                    let (result, more) = again(first, || lower(true, &mut None));
                    recovered = more;
                    (shell.source, result)
                }
                Pending::Method(index) => {
                    let definition = &program.methods[index];
                    let body_source = source_span(
                        definition
                            .body_text()
                            .unwrap_or_else(|| program.source.text_at(definition.source())),
                        definition.body(),
                    );
                    let mut lower =
                        |recover: bool, failure: &mut Option<rawr_import::surface::Error>| {
                            let mut parser = match Parser::for_method(
                                engine,
                                &body_source,
                                models.for_source(definition.source()),
                                definition,
                                0,
                                0,
                                Some(ImportedContext {
                                    native: None,
                                    allocation: &mut *allocation,
                                    failure,
                                    source: definition.source(),
                                    uses: &mut uses,
                                    dependencies: &mut dependencies,
                                }),
                            ) {
                                Ok(parser) => parser,
                                Err(error) => return (Err(error), Vec::new()),
                            };
                            parser.symbolic_only = true;
                            parser.recover_misses = recover;
                            let output = definition.output();
                            let result = parser.opaque_expected(output).and_then(|expected| {
                                parser.return_ty = Some(expected.clone());
                                parser.revealing = Some(expected.clone());
                                parser.parse_method_body(definition)?;
                                parser.fix_opaque(output, &expected)?;
                                let hidden = parser.inference.resolve(&expected);
                                named_output(output, &hidden, &parser.models)
                            });
                            (result, std::mem::take(&mut parser.recovered))
                        };
                    let first = lower(false, &mut allocation_failure);
                    let (result, more) = again(first, || lower(true, &mut None));
                    recovered = more;
                    (definition.source(), result)
                }
            };
            match recovered_result(result, recovered, &mut allocation_failure, source) {
                Ok(output) => revealed.push((next, output)),
                Err(crate::resolve::ImportedFailure::Surface(error)) => {
                    return Err(crate::resolve::ImportedFailure::Surface(error));
                }
                // A body that passes another body's opaque output to a
                // crate before that output is revealed, as multi_window's
                // `center(window.view(id))` does when `Window::view` comes
                // later, waits for it. When nothing reveals, a failure that
                // names an item Rawr doesn't know comes first: the crate is
                // asked for it, after which `Window::view` may reveal.
                Err(crate::resolve::ImportedFailure::Lowering(failure)) => {
                    waiting.push(next);
                    if failure.names_unknown() {
                        // Every body's miss is asked for at once (#662).
                        match &mut first_unknown {
                            Some(first) => LoweringFailure::also(first, failure),
                            None => first_unknown = Some(failure),
                        }
                    } else {
                        match &mut first_failure {
                            Some(first) => LoweringFailure::also(first, failure),
                            None => first_failure = Some(failure),
                        }
                    }
                }
            }
        }
        // A round that reveals nothing and adds no instance has no other
        // reveal to wait for.
        if revealed.is_empty() && added.is_empty() {
            return Err(crate::resolve::ImportedFailure::Lowering(
                match (first_unknown, first_failure) {
                    (Some(mut unknown), Some(failure)) => {
                        unknown.also(failure);
                        Some(unknown)
                    }
                    (unknown, failure) => unknown.or(failure),
                }
                .expect("an unrevealed round keeps its failure"),
            ));
        }
        for (next, output) in revealed {
            match next {
                Pending::Function(index) => {
                    if let item::Kind::Function(definition) = &mut program.items[index].kind
                        && let Some(written) = definition.output.replace(output)
                    {
                        definition.lifetimes.written_output = Some(Box::new(written));
                    }
                }
                Pending::Method(index) => program.methods[index].reveal_output(output),
            }
        }
        pending = waiting;
    }
    Ok(())
}

/// `written` with each opaque `impl Trait` replaced by the hidden type the
/// body bound in its place (`hidden` is the whole output, resolved).
#[cfg(feature = "rust-import")]
pub(super) fn named_output(
    written: &type_ref::Type,
    hidden: &Ty,
    models: &Models,
) -> Result<type_ref::Type, Error> {
    let span = match written {
        type_ref::Type::Unit(span)
        | type_ref::Type::Tuple { span, .. }
        | type_ref::Type::Reference { span, .. } => *span,
        type_ref::Type::Path(path) => path.span,
    };
    if let Some(trait_name) = written.opaque_trait() {
        return named_type(hidden, models, span).ok_or_else(|| Error::UnsupportedNominalSyntax {
            construct: format!(
                "`impl {trait_name}` whose body returns `{}`, a type that can't be named in a \
                 signature,",
                hidden.name(models)
            )
            .into(),
            source: span,
        });
    }
    if !written.has_opaque() {
        return Ok(written.clone());
    }
    Ok(match (written, hidden) {
        (type_ref::Type::Tuple { members, span }, Ty::Tuple(hidden)) => type_ref::Type::Tuple {
            members: members
                .iter()
                .zip(hidden.iter())
                .map(|(member, hidden)| named_output(member, hidden, models))
                .collect::<Result<_, _>>()?,
            span: *span,
        },
        (
            type_ref::Type::Reference {
                mutable,
                lifetime,
                target,
                span,
            },
            Ty::Reference { target: hidden, .. },
        ) => type_ref::Type::Reference {
            mutable: *mutable,
            lifetime: *lifetime,
            target: Box::new(named_output(target, hidden, models)?),
            span: *span,
        },
        (type_ref::Type::Path(path), _) => {
            let mut inner = match hidden {
                Ty::Option(element)
                | Ty::Array { element, .. }
                | Ty::Boxed(element)
                | Ty::Counted(_, element)
                | Ty::Weak(_, element) => {
                    vec![&**element]
                }
                Ty::Result(ok, err) => vec![&**ok, &**err],
                Ty::Host { arguments, .. } | Ty::Script(_, arguments) => arguments.iter().collect(),
                _ => Vec::new(),
            }
            .into_iter();
            let mut arguments = Vec::with_capacity(path.arguments.len());
            for argument in &path.arguments {
                arguments.push(match argument {
                    type_ref::Argument::Type(ty) => {
                        let hidden = inner.next().unwrap_or(&Ty::Unknown);
                        type_ref::Argument::Type(named_output(ty, hidden, models)?)
                    }
                    type_ref::Argument::Lifetime(span) => type_ref::Argument::Lifetime(*span),
                });
            }
            type_ref::Type::Path(type_ref::Path {
                arguments: arguments.into_boxed_slice(),
                ..path.clone()
            })
        }
        _ => written.clone(),
    })
}

/// The header type naming `ty`, a body's resolved type, as a signature
/// writes it: `None` for a type no signature names (a closure, an
/// iterator chain) or one still unknown.
#[cfg(feature = "rust-import")]
pub(super) fn named_type(ty: &Ty, models: &Models, span: Span) -> Option<type_ref::Type> {
    let builtin = |name: &str, arguments: Vec<type_ref::Type>| {
        type_ref::Type::Path(type_ref::Path {
            name: name.into(),
            target: type_ref::Target::Builtin,
            arguments: arguments
                .into_iter()
                .map(type_ref::Argument::Type)
                .collect(),
            span,
        })
    };
    let named = |ty: &Ty| named_type(ty, models, span);
    let all = |types: &[Ty]| types.iter().map(named).collect::<Option<Vec<_>>>();
    Some(match ty {
        Ty::Unit => type_ref::Type::Unit(span),
        Ty::Bool => builtin("bool", Vec::new()),
        Ty::Char => builtin("char", Vec::new()),
        Ty::String => builtin("String", Vec::new()),
        // A `&str` is the `String` path with the reference's lifetime.
        Ty::Str => type_ref::Type::Path(type_ref::Path {
            name: "String".into(),
            target: type_ref::Target::Builtin,
            arguments: Box::new([type_ref::Argument::Lifetime(span)]),
            span,
        }),
        // An unsuffixed literal falls back to `i32` or `f64`, as in rustc.
        Ty::Int(Int::Var(_)) => builtin("i32", Vec::new()),
        Ty::Int(kind) => builtin(kind.name(), Vec::new()),
        Ty::Float(Float::Var(_)) => builtin("f64", Vec::new()),
        Ty::Float(kind) => builtin(kind.name(), Vec::new()),
        // std's types Rawr knows by name, as iced's `Never` is std's
        // `Infallible`, a message no widget sends.
        Ty::ParseError(error) => builtin(error.name(), Vec::new()),
        Ty::Ordering => builtin("Ordering", Vec::new()),
        Ty::Time(time) => builtin(time.name(), Vec::new()),
        Ty::Option(element) => builtin("Option", vec![named(element)?]),
        Ty::Result(ok, err) => builtin("Result", vec![named(ok)?, named(err)?]),
        Ty::Array { element, kind } => builtin(
            &match kind {
                sequence::Kind::Array(length) => crate::item::type_ref::array_name(*length),
                sequence::Kind::Vec => "Vec".into(),
                sequence::Kind::Slice => crate::item::type_ref::SLICE_TYPE.into(),
                sequence::Kind::Deque => "VecDeque".into(),
            },
            vec![named(element)?],
        ),
        Ty::Boxed(target) => builtin("Box", vec![named(target)?]),
        Ty::Counted(counter, target) => builtin(counter.name(), vec![named(target)?]),
        Ty::Weak(counter, target) => builtin(counter.weak_name(), vec![named(target)?]),
        Ty::Tuple(members) => type_ref::Type::Tuple {
            members: all(members)?.into_boxed_slice(),
            span,
        },
        Ty::Reference { mutable, target } => type_ref::Type::Reference {
            mutable: *mutable,
            lifetime: None,
            target: Box::new(named(target)?),
            span,
        },
        Ty::Host { proof, arguments } => type_ref::Type::Path(type_ref::Path {
            name: proof.definition.path().into(),
            target: type_ref::Target::Host {
                definition: proof.definition.clone(),
                owner: proof.owner.clone(),
            },
            arguments: arguments
                .iter()
                .map(|argument| match argument {
                    // A closure a crate value holds, as iced's `Map<W, F,
                    // A>` from `Widget::map` does: `fn(A) -> B`, as the
                    // bridge boxes it (T417).
                    Ty::Callable { inputs, output, .. }
                    | Ty::Fn { inputs, output }
                    | Ty::FnItem { inputs, output, .. }
                    | Ty::FnChoice { inputs, output, .. } => {
                        let mut types = all(inputs)?;
                        types.push(named(output)?);
                        Some(builtin(crate::item::type_ref::FN_TYPE, types))
                    }
                    argument => named(argument),
                })
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .map(type_ref::Argument::Type)
                .collect(),
            span,
        }),
        Ty::Script(definition, arguments) => {
            let declared = models.definition(*definition).definition();
            type_ref::Type::Path(type_ref::Path {
                name: declared.name().into(),
                target: type_ref::Target::Script {
                    definition: *definition,
                    identity: type_def::Identity::Script(declared.key().clone()),
                },
                arguments: all(arguments)?
                    .into_iter()
                    .map(type_ref::Argument::Type)
                    .collect(),
                span,
            })
        }
        _ => return None,
    })
}

/// Whether rawr compiles `function` once per instantiation its callers use:
/// a generic parameter of its touches a crate's types, by its signature
/// naming it inside a crate type or by a bound naming a crate's trait
/// (D629).
#[cfg(feature = "rust-import")]
pub(super) fn instanced(function: &item::Function) -> bool {
    function.generics.iter().any(|generic| generic.host)
}

/// Whether rawr compiles the script method `definition` once per
/// instantiation its callers use, as `instanced` decides for a function: a
/// generic parameter of its impl or its own touches a crate's types (D629).
#[cfg(feature = "rust-import")]
pub(super) fn instanced_method(definition: &method::Definition) -> bool {
    definition.trait_name().is_none()
        && definition.builtin_owner().is_none()
        && definition.generics().iter().any(|generic| generic.host)
}

/// Whether rawr compiles the `from` of a script `impl From<S>` for a
/// crate's type once per instantiation its callers use: the impl's own
/// parameter is one of the crate type's arguments, as `M` of `impl<'a, M>
/// From<Mine> for Slot<'a, M>`, which the crate takes at one type at each
/// use (D629). Each conversion binds its instance (`select_conversion`).
#[cfg(feature = "rust-import")]
pub(super) fn instanced_crate_impl(definition: &method::Definition) -> bool {
    definition.conversion().is_some()
        && definition.builtin_owner().is_some_and(|owner| {
            matches!(
                owner,
                type_ref::Type::Path(type_ref::Path {
                    target: type_ref::Target::Host { .. },
                    ..
                })
            ) && mentions_parameter(owner)
        })
}

/// An item `instance_round` added: a function item's index, or a method's.
#[cfg(feature = "rust-import")]
#[derive(Clone, Copy)]
pub(super) enum Added {
    Function(usize),
    Method(usize),
}

/// The names of `types`, which key an instance, when each is a type a
/// signature can name.
#[cfg(feature = "rust-import")]
pub(super) fn instance_names(types: &[Ty], models: &Models) -> Option<Vec<ImmutableString>> {
    types
        .iter()
        .map(|ty| {
            named_type(ty, models, Span::NONE)?;
            Some(ty.name(models).to_string().into())
        })
        .collect()
}

/// `ty` with each generic parameter `types` names replaced by its type.
#[cfg(feature = "rust-import")]
pub(super) fn substitute_parameters(
    ty: &type_ref::Type,
    types: &[(ImmutableString, type_ref::Type)],
) -> type_ref::Type {
    match ty {
        type_ref::Type::Path(path) if matches!(path.target, type_ref::Target::Parameter) => {
            if let Some((_, replaced)) = types.iter().find(|(name, _)| *name == path.name) {
                return replaced.clone();
            }
            ty.clone()
        }
        type_ref::Type::Path(path) => type_ref::Type::Path(type_ref::Path {
            arguments: path
                .arguments
                .iter()
                .map(|argument| match argument {
                    type_ref::Argument::Type(ty) => {
                        type_ref::Argument::Type(substitute_parameters(ty, types))
                    }
                    lifetime @ type_ref::Argument::Lifetime(_) => lifetime.clone(),
                })
                .collect(),
            ..path.clone()
        }),
        type_ref::Type::Reference {
            mutable,
            lifetime,
            target,
            span,
        } => type_ref::Type::Reference {
            mutable: *mutable,
            lifetime: *lifetime,
            target: Box::new(substitute_parameters(target, types)),
            span: *span,
        },
        type_ref::Type::Tuple { members, span } => type_ref::Type::Tuple {
            members: members
                .iter()
                .map(|member| substitute_parameters(member, types))
                .collect(),
            span: *span,
        },
        type_ref::Type::Unit(span) => type_ref::Type::Unit(*span),
    }
}

/// The calls of generic functions compiled per instantiation that a body
/// makes, with the types each gives, resolved once the body is read, and
/// each call's span.
#[cfg(feature = "rust-import")]
pub(super) fn take_instance_calls(
    parser: &mut Parser<'_>,
) -> Vec<(crate::item::Key, Vec<Ty>, Span)> {
    std::mem::take(&mut parser.instance_calls)
        .into_iter()
        .map(|(key, types, call)| {
            let types = types
                .iter()
                .map(|ty| parser.inference.resolve(ty))
                .collect::<Vec<_>>();
            (key, types, call)
        })
        // A literal still open is one of a body that stopped before its
        // end, which settles it: `50` may be the `u32` a bound's impl
        // picks, so its call waits for a reading that settles it.
        .filter(|(_, types, _)| !types.iter().any(contains_open_literal))
        .collect()
}

/// A use of a script impl of a crate's trait (`Parser::impl_uses`): the
/// impl item's index, its parameters' types, and the trait's arguments
/// with its defaults filled in.
pub(super) type ImplUse = (usize, Vec<(ImmutableString, Ty)>, UseArguments);

/// A trait's arguments at a use of a script impl, its defaults filled in.
pub(super) type UseArguments = Vec<Ty>;

/// The uses a body makes of script impls of crate traits (`Parser::
/// impl_uses`), resolved once the body is read.
#[cfg(feature = "rust-import")]
pub(super) fn take_impl_uses(parser: &mut Parser<'_>) -> Vec<ImplUse> {
    std::mem::take(&mut parser.impl_uses)
        .into_iter()
        .map(|(index, types, arguments)| {
            let types = types
                .into_iter()
                .map(|(name, ty)| (name, parser.inference.resolve(&ty)))
                .collect();
            let arguments = arguments
                .iter()
                .map(|ty| parser.inference.resolve(ty))
                .collect();
            (index, types, arguments)
        })
        .collect()
}

/// Compiles each generic function that touches a crate's types once per
/// instantiation its checked callers use, as Rust monomorphises it (D629).
/// Every body but the templates' is read for its calls, round by round
/// beside `reveal`'s, as an instance's own calls may need instances in turn
/// and a call's types may come from a revealed output. Each instance is a
/// function item of its own, whose signature names the types its template's
/// parameters take and whose body reads them as those types. Each call
/// binds to its instance (`bind_instance`), and a template's own body is
/// never lowered.
/// Lists each template (`instanced`), and whether there is one.
#[cfg(feature = "rust-import")]
pub(super) fn instance_templates(program: &mut Program) -> bool {
    let templates = program
        .items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Function(definition) if instanced(definition) => shell.key.clone(),
            _ => None,
        })
        .chain(
            program
                .methods
                .iter()
                .filter(|definition| instanced_method(definition))
                .map(|definition| definition.key().clone()),
        )
        .chain(
            program
                .builtin_methods
                .iter()
                .filter(|definition| instanced_crate_impl(definition))
                .map(|definition| definition.key().clone()),
        )
        .collect::<Vec<_>>();
    for template in &templates {
        program
            .instances
            .templates
            .entry(template.clone())
            .or_default();
    }
    // A script impl of a crate's trait whose own parameters touch a crate's
    // types (`impl<R: renderer::Renderer> Widget<M, T, R> for Quad`): its
    // functions are templates, compiled for each use a crate's bound makes
    // of the impl.
    for (index, shell) in program.items.iter().enumerate() {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        // Its parameters touch a crate's types where a crate's trait bounds
        // one, or where a function's signature names one, which the runner
        // writes at a type (component's `update(..) -> Option<Message>`).
        let signatures = || {
            implementation.functions.iter().any(|function| {
                function
                    .parameters
                    .iter()
                    .map(|parameter| &parameter.ty)
                    .chain(function.output.as_ref())
                    .any(mentions_parameter)
            })
        };
        if !matches!(implementation.trait_, Some(item::TraitImpl::Native { .. }))
            || implementation.generics.is_empty()
            || !(implementation.generics.iter().any(|generic| generic.host) || signatures())
        {
            continue;
        }
        program.instances.impls.entry(index).or_default();
        for function in implementation.functions.iter() {
            if let item::Lowering::NominalMethod(id) = function.body.lowering
                && let Some(definition) = program.methods.get(id.index())
            {
                program
                    .instances
                    .templates
                    .entry(definition.key().clone())
                    .or_default();
            }
        }
    }
    !program.instances.templates.is_empty()
}

/// The generic functions and methods whose signatures name no crate type
/// but whose bodies pass a generic parameter of theirs to a crate's item, or
/// to a generic compiled per instantiation: they touch a crate's types only
/// in their bodies, and are compiled per instantiation too (D629). Each is
/// read once with its parameters erased, and again while another joins the
/// templates. Whether any template is listed.
#[cfg(feature = "rust-import")]
pub(super) fn body_templates<A: crate::host::ImportAllocation>(
    engine: &Engine,
    program: &mut Program,
    host: &Shared<crate::host::Registry>,
    allocation: &mut A,
) -> Result<bool, crate::resolve::ImportedFailure> {
    // A function's or method's key, with its index among its kind.
    let mut candidates = program
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, shell)| match &shell.kind {
            item::Kind::Function(definition) if !definition.generics.is_empty() => {
                shell.key.clone().map(|key| (key, Added::Function(index)))
            }
            _ => None,
        })
        .chain(
            program
                .methods
                .iter()
                .enumerate()
                .filter(|(_, definition)| {
                    !definition.generics().is_empty()
                        && definition.trait_name().is_none()
                        && definition.builtin_owner().is_none()
                })
                .map(|(index, definition)| (definition.key().clone(), Added::Method(index))),
        )
        .filter(|(key, _)| !program.instances.templates.contains_key(key))
        .collect::<Vec<_>>();
    for _ in 0..MAX_INSTANCE_ROUNDS {
        if candidates.is_empty() {
            break;
        }
        let runtime =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), program);
        let models = Models::new(program, runtime, Shared::clone(host), engine);
        let mut promoted = Vec::new();
        for (key, candidate) in &candidates {
            let mut uses = Vec::new();
            let mut dependencies = BTreeSet::new();
            // An erased body may fail where a crate's item takes its
            // parameter; it fails again where it is lowered, if it isn't
            // compiled per instantiation.
            let mut allocation_failure = None;
            let source = match *candidate {
                Added::Function(index) => program.items[index].source,
                Added::Method(index) => program.methods[index].source(),
            };
            let context = ImportedContext {
                native: None,
                allocation: &mut *allocation,
                failure: &mut allocation_failure,
                source,
                uses: &mut uses,
                dependencies: &mut dependencies,
            };
            let reaches = match *candidate {
                Added::Function(index) => {
                    let shell = &program.items[index];
                    let item::Kind::Function(definition) = &shell.kind else {
                        continue;
                    };
                    let body_source = body_source(
                        definition
                            .body
                            .text
                            .as_deref()
                            .unwrap_or_else(|| program.source.text_at(shell.source)),
                        &definition.body,
                    );
                    let mut parser = Parser::for_function(
                        engine,
                        &body_source,
                        models.for_source(shell.source),
                        definition,
                        key.clone(),
                        0,
                        0,
                        Some(context),
                    );
                    parser.symbolic_only = true;
                    parser.collecting_instances = true;
                    let _ = parser
                        .expect_output(definition.output.as_ref())
                        .and_then(|()| parser.parse_function_body(definition));
                    parser.reaches_host
                }
                Added::Method(index) => {
                    let definition = &program.methods[index];
                    let body_source = source_span(
                        definition
                            .body_text()
                            .unwrap_or_else(|| program.source.text_at(definition.source())),
                        definition.body(),
                    );
                    match Parser::for_method(
                        engine,
                        &body_source,
                        models.for_source(definition.source()),
                        definition,
                        0,
                        0,
                        Some(context),
                    ) {
                        Ok(mut parser) => {
                            parser.symbolic_only = true;
                            parser.collecting_instances = true;
                            let _ = parser
                                .expect_output(Some(definition.output()))
                                .and_then(|()| parser.parse_method_body(definition));
                            parser.reaches_host
                        }
                        Err(_) => false,
                    }
                }
            };
            if reaches {
                promoted.push(key.clone());
            }
        }
        if promoted.is_empty() {
            break;
        }
        for key in &promoted {
            program.instances.templates.entry(key.clone()).or_default();
        }
        candidates.retain(|(key, _)| !promoted.contains(key));
    }
    Ok(!program.instances.templates.is_empty())
}

/// The instance `id` of the method template `definition` at `types`, the
/// types of its generic parameters, whose names are `names`: listed among
/// the template's instances, its body reading each parameter as its type.
#[cfg(feature = "rust-import")]
pub(super) fn method_instance(
    instances: &mut crate::resolve::Instances,
    models: &Models,
    definition: &method::Definition,
    names: Vec<ImmutableString>,
    types: &[Ty],
    id: method::Id,
) -> method::Definition {
    let template = definition.key();
    let listed = instances
        .templates
        .get_mut(template)
        .expect("each template is listed");
    // A trait impl's function keeps its trait in its name, by which native
    // code reaches it (`Widget<M, T, R>::draw#1`).
    let name: ImmutableString = format!("{}#{}", template.name(), listed.len() + 1).into();
    let key = template.renamed(name.clone());
    listed.push((names.into_boxed_slice(), key.clone()));
    let span = definition.name_span();
    let substituted = definition
        .generics()
        .iter()
        .zip(types)
        .map(|(generic, ty)| {
            let named = named_type(ty, models, span)
                .expect("an instance's types are each one a signature names");
            (generic.name.text.clone(), named)
        })
        .collect::<Box<[_]>>();
    let instance = definition.instance(id, key.clone(), name, |ty| {
        substitute_parameters(ty, &substituted)
    });
    instances.types.insert(key, substituted);
    instance
}

/// Polymorphic recursion would make instances without end.
#[cfg(feature = "rust-import")]
pub(super) const MAX_INSTANCE_ROUNDS: usize = 32;

/// One round of `monomorphize`: the instance items and methods added.
#[cfg(feature = "rust-import")]
pub(super) fn instance_round<A: crate::host::ImportAllocation>(
    engine: &Engine,
    program: &mut Program,
    host: &Shared<crate::host::Registry>,
    allocation: &mut A,
) -> Result<Vec<Added>, crate::resolve::ImportedFailure> {
    let templates = program
        .instances
        .templates
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    {
        let runtime =
            crate::program::model::Model::new(crate::program::GenerationId::issue(), program);
        let models = Models::new(program, runtime, Shared::clone(host), engine);
        // Each call, by the body that makes it: the template and its types.
        let mut found = Vec::<(crate::item::Key, crate::item::Key, Vec<Ty>, Span)>::new();
        let mut impl_uses = Vec::new();
        for shell in program.items.iter() {
            let item::Kind::Function(definition) = &shell.kind else {
                continue;
            };
            let Some(key) = shell.key.clone() else {
                continue;
            };
            if templates.contains(&key) {
                continue;
            }
            let body_source = body_source(
                definition
                    .body
                    .text
                    .as_deref()
                    .unwrap_or_else(|| program.source.text_at(shell.source)),
                &definition.body,
            );
            let mut uses = Vec::new();
            let mut dependencies = BTreeSet::new();
            let mut allocation_failure = None;
            let mut parser = Parser::for_function(
                engine,
                &body_source,
                models.for_source(shell.source),
                definition,
                key.clone(),
                0,
                0,
                Some(ImportedContext {
                    native: None,
                    allocation,
                    failure: &mut allocation_failure,
                    source: shell.source,
                    uses: &mut uses,
                    dependencies: &mut dependencies,
                }),
            );
            parser.symbolic_only = true;
            parser.collecting_instances = true;
            // A body that fails here fails again where it is lowered.
            let _ = parser.parse_function_body(definition);
            found.extend(
                take_instance_calls(&mut parser)
                    .into_iter()
                    .map(|(template, types, call)| (key.clone(), template, types, call)),
            );
            impl_uses.extend(take_impl_uses(&mut parser));
            drop(parser);
            if let Some(error) = allocation_failure {
                return Err(crate::resolve::ImportedFailure::Surface(error));
            }
        }
        for definition in program.methods.iter() {
            if templates.contains(definition.key()) {
                continue;
            }
            let body_source = source_span(
                definition
                    .body_text()
                    .unwrap_or_else(|| program.source.text_at(definition.source())),
                definition.body(),
            );
            let mut uses = Vec::new();
            let mut dependencies = BTreeSet::new();
            let mut allocation_failure = None;
            let parser = Parser::for_method(
                engine,
                &body_source,
                models.for_source(definition.source()),
                definition,
                0,
                0,
                Some(ImportedContext {
                    native: None,
                    allocation,
                    failure: &mut allocation_failure,
                    source: definition.source(),
                    uses: &mut uses,
                    dependencies: &mut dependencies,
                }),
            );
            if let Ok(mut parser) = parser {
                parser.symbolic_only = true;
                parser.collecting_instances = true;
                let _ = parser.parse_method_body(definition);
                found.extend(take_instance_calls(&mut parser).into_iter().map(
                    |(template, types, call)| (definition.key().clone(), template, types, call),
                ));
                impl_uses.extend(take_impl_uses(&mut parser));
            }
            if let Some(error) = allocation_failure {
                return Err(crate::resolve::ImportedFailure::Surface(error));
            }
        }
        let mut added = Vec::<(crate::item::Key, Vec<ImmutableString>, Vec<Ty>)>::new();
        // Two calls a macro gives one span may take two instances: neither
        // is known by its span.
        let mut calls = BTreeMap::<(crate::item::Key, Span), Option<Box<[ImmutableString]>>>::new();
        for (caller, template, types, call) in found {
            let Some(names) = instance_names(&types, &models) else {
                continue;
            };
            calls
                .entry((caller, call))
                .and_modify(|known| {
                    if known.as_deref() != Some(names.as_slice()) {
                        *known = None;
                    }
                })
                .or_insert_with(|| Some(names.clone().into_boxed_slice()));
            let Some(instances) = program.instances.templates.get(&template) else {
                continue;
            };
            let known = instances
                .iter()
                .any(|(written, _)| written.as_ref() == names.as_slice())
                || added
                    .iter()
                    .any(|(key, written, _)| *key == template && *written == names);
            if !known {
                added.push((template, names, types));
            }
        }
        program.instances.calls = calls
            .into_iter()
            .filter_map(|(call, names)| Some((call, names?)))
            .collect();
        // Each new use of a script impl of a crate's trait whose parameters
        // touch a crate's types is an instance of the impl, and of each of
        // its functions.
        for (index, types, arguments) in impl_uses {
            let Some(known) = program.instances.impls.get(&index) else {
                continue;
            };
            let tys = types.iter().map(|(_, ty)| ty.clone()).collect::<Vec<_>>();
            let Some(names) = instance_names(&tys, &models) else {
                continue;
            };
            if known
                .iter()
                .any(|instance| instance.names.as_ref() == names.as_slice())
            {
                continue;
            }
            let span = Span::NONE;
            let Some(named) = types
                .iter()
                .map(|(name, ty)| named_type(ty, &models, span).map(|ty| (name.clone(), ty)))
                .collect::<Option<Box<[_]>>>()
            else {
                continue;
            };
            let Some(arguments) = arguments
                .iter()
                .map(|ty| named_type(ty, &models, span))
                .collect::<Option<Box<[_]>>>()
            else {
                continue;
            };
            let item::Kind::Impl(implementation) = &program.items[index].kind else {
                continue;
            };
            for function in implementation.functions.iter() {
                if let item::Lowering::NominalMethod(id) = function.body.lowering
                    && let Some(definition) = program.methods.get(id.index())
                {
                    added.push((definition.key().clone(), names.clone(), tys.clone()));
                }
            }
            program
                .instances
                .impls
                .get_mut(&index)
                .expect("a known impl template")
                .push(crate::resolve::ImplInstance {
                    names: names.into_boxed_slice(),
                    types: named,
                    arguments,
                });
        }
        if added.is_empty() {
            return Ok(Vec::new());
        }
        // A function of an impl for a crate's type has its instances beside
        // it, numbered as such impls' functions are.
        let (added_builtin, added): (Vec<_>, Vec<_>) = added.into_iter().partition(|(key, ..)| {
            program
                .builtin_methods
                .iter()
                .any(|definition| definition.key() == key)
        });
        let mut builtin_methods = std::mem::take(&mut program.builtin_methods).into_vec();
        for (template, names, types) in added_builtin {
            let definition = builtin_methods
                .iter()
                .find(|definition| *definition.key() == template)
                .expect("a builtin template is an impl's function")
                .clone();
            let id =
                method::Id::from_index(crate::resolve::BUILTIN_METHOD_IDS + builtin_methods.len());
            builtin_methods.push(method_instance(
                &mut program.instances,
                &models,
                &definition,
                names,
                &types,
                id,
            ));
        }
        program.builtin_methods = builtin_methods.into_boxed_slice();
        // A method's instance is a method of its owner, beside the others.
        let (added_methods, added): (Vec<_>, Vec<_>) = added.into_iter().partition(|(key, ..)| {
            program
                .methods
                .iter()
                .any(|definition| definition.key() == key)
        });
        let mut methods = std::mem::take(&mut program.methods).into_vec();
        let first_method = methods.len();
        for (template, names, types) in added_methods {
            let definition = methods
                .iter()
                .find(|definition| *definition.key() == template)
                .expect("a method template is a method")
                .clone();
            let id = method::Id::from_index(methods.len());
            methods.push(method_instance(
                &mut program.instances,
                &models,
                &definition,
                names,
                &types,
                id,
            ));
        }
        let added_methods = (first_method..methods.len()).map(Added::Method);
        program.methods = methods.into_boxed_slice();
        let mut items = std::mem::take(&mut program.items).into_vec();
        let first = items.len();
        let next = items
            .iter()
            .map(|shell| shell.id.index())
            .max()
            .map_or(0, |index| index + 1);
        for (offset, (template, names, types)) in added.into_iter().enumerate() {
            let shell = items
                .iter()
                .find(|shell| shell.key.as_ref() == Some(&template))
                .expect("a template is a function item")
                .clone();
            let item::Kind::Function(definition) = &shell.kind else {
                unreachable!("templates are function items");
            };
            let instances = program
                .instances
                .templates
                .get_mut(&template)
                .expect("each template is listed");
            let name: ImmutableString =
                format!("{}#{}", definition.name.text, instances.len() + 1).into();
            let key = crate::item::Key::new(
                template.package().into(),
                template.modules().to_vec().into_boxed_slice(),
                name.clone(),
                crate::item::Kind::Function,
            );
            instances.push((names.into_boxed_slice(), key.clone()));
            let span = definition.name.span;
            let substituted = definition
                .generics
                .iter()
                .zip(&types)
                .map(|(generic, ty)| {
                    let named = named_type(ty, &models, span)
                        .expect("an instance's types are each one a signature names");
                    (generic.name.text.clone(), named)
                })
                .collect::<Box<[_]>>();
            let mut instance = definition.clone();
            instance.name.text = name;
            instance.generics = Box::default();
            for parameter in instance.parameters.iter_mut() {
                parameter.ty = substitute_parameters(&parameter.ty, &substituted);
            }
            instance.output = instance
                .output
                .as_ref()
                .map(|output| substitute_parameters(output, &substituted));
            // Elision reads the template's output, as rustc reads the
            // generic signature (`method::Definition::instance`).
            if instance.lifetimes.written_output.is_none() {
                instance.lifetimes.written_output = definition.output.clone().map(Box::new);
            }
            instance.lifetimes.bounds = instance
                .lifetimes
                .bounds
                .iter()
                .filter(|(bounded, _)| !substituted.iter().any(|(name, _)| name == bounded))
                .cloned()
                .collect();
            program.instances.types.insert(key.clone(), substituted);
            items.push(item::Shell {
                id: crate::item::Id::from_index(next + offset),
                key: Some(key),
                kind: item::Kind::Function(instance),
                ..shell
            });
        }
        let indices = (first..items.len())
            .map(Added::Function)
            .chain(added_methods)
            .collect();
        program.items = items.into_boxed_slice();
        Ok(indices)
    }
}

#[cfg(feature = "rust-import")]
pub(crate) struct ImportedOutput {
    pub(crate) uses: Box<[ImportedUse]>,
    /// The script's impls of dependencies' traits, by the script path of
    /// the type each is for (D594).
    pub(crate) impls: Box<[(Box<str>, rawr_import::surface::instance::ScriptImpl)]>,
}

/// A body's lowering in a pass that goes on past misses (#662), as
/// `recovered_result` and then `missing` take it.
#[cfg(feature = "rust-import")]
pub(super) fn body<T>(
    lowered: Result<T, Error>,
    more: Vec<Error>,
    allocation_failure: &mut Option<rawr_import::surface::Error>,
    source: crate::program::source::Id,
    missed: &mut Option<LoweringFailure>,
) -> Result<Option<T>, crate::resolve::ImportedFailure> {
    missing(
        recovered_result(lowered, more, allocation_failure, source),
        missed,
    )
}

/// A body's first lowering is its result. One that missed an item Rawr
/// doesn't know is lowered once more, going on past each miss
/// (`recover_miss`), for what else it misses (#662). Only the first
/// lowering decides: going on past a miss would also take a candidate
/// that the first lowering tried and set aside. A first lowering that
/// failed otherwise keeps what it missed on the way, as the `deref` a
/// coercion through a crate type looked for.
#[cfg(feature = "rust-import")]
pub(super) fn again<T>(
    (lowered, missed): (Result<T, Error>, Vec<Error>),
    recover: impl FnOnce() -> (Result<T, Error>, Vec<Error>),
) -> (Result<T, Error>, Vec<Error>) {
    match lowered {
        Err(error) if LoweringFailure::miss_of(&error).is_some() => {
            let (second, mut more) = recover();
            more.extend(second.err());
            more.retain(|other| *other != error);
            (Err(error), more)
        }
        Err(error) => (Err(error), missed),
        lowered => (lowered, Vec::new()),
    }
}

/// A body's lowering, with what a second lowering of it missed besides
/// (`again`) kept beside its failure.
#[cfg(feature = "rust-import")]
pub(super) fn recovered_result<T>(
    lowered: Result<T, Error>,
    more: Vec<Error>,
    allocation_failure: &mut Option<rawr_import::surface::Error>,
    source: crate::program::source::Id,
) -> Result<T, crate::resolve::ImportedFailure> {
    match imported_result(lowered, allocation_failure, source) {
        Err(crate::resolve::ImportedFailure::Lowering(mut failure)) => {
            failure.keep(more);
            Err(crate::resolve::ImportedFailure::Lowering(failure))
        }
        result => result,
    }
}

/// A body's result in a pass that goes on past misses (#662): `None` for a
/// body that failed once an earlier body missed an item Rawr doesn't know,
/// whose miss is kept with that one when it names one too. The first
/// failure is the pass's error, as when the pass stopped at it.
#[cfg(feature = "rust-import")]
pub(super) fn missing<T>(
    result: Result<T, crate::resolve::ImportedFailure>,
    missed: &mut Option<LoweringFailure>,
) -> Result<Option<T>, crate::resolve::ImportedFailure> {
    match (result, missed.as_mut()) {
        (Ok(value), _) => Ok(Some(value)),
        (Err(crate::resolve::ImportedFailure::Lowering(failure)), Some(first)) => {
            first.also(failure);
            Ok(None)
        }
        (Err(crate::resolve::ImportedFailure::Lowering(failure)), None) if failure.missed() => {
            *missed = Some(failure);
            Ok(None)
        }
        (Err(failure), _) => Err(failure),
    }
}

#[cfg(feature = "rust-import")]
pub(super) fn imported_result<T>(
    result: Result<T, Error>,
    allocation_failure: &mut Option<rawr_import::surface::Error>,
    source: crate::program::source::Id,
) -> Result<T, crate::resolve::ImportedFailure> {
    match result {
        Ok(value) => Ok(value),
        Err(_) if allocation_failure.is_some() => Err(crate::resolve::ImportedFailure::Surface(
            allocation_failure
                .take()
                .expect("guard proves a retained allocation failure"),
        )),
        Err(error) => Err(crate::resolve::ImportedFailure::Lowering(
            LoweringFailure::original(source, error.into()),
        )),
    }
}

/// The script's impls of a dependency's traits for its own types (D594),
/// each with the script path of its owner, which crosses into native code
/// as its proxy: `impl canvas::Program<Message> for Arc`. Native code calls
/// each function through the runner's impl for the proxy.
#[cfg(feature = "rust-import")]
pub(super) fn native_impls(
    program: &Program,
    models: &Models,
) -> Result<
    Vec<(Box<str>, rawr_import::surface::instance::ScriptImpl)>,
    crate::resolve::ImportedFailure,
> {
    let mut impls = Vec::new();
    for (index, shell) in program.items.iter().enumerate() {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        let script = |ty: &type_ref::Type| match ty {
            type_ref::Type::Path(type_ref::Path {
                target: type_ref::Target::Script { definition, .. },
                ..
            }) => Some(*definition),
            _ => None,
        };
        // `impl canvas::Program<Message> for Arc`, of a dependency's trait
        // for a script type; or `impl From<Shade> for iced::Color`, of the
        // prelude's conversion from a script type into a crate's, which a
        // crate's `impl Into<Color>` takes. Each crosses with its script
        // type.
        let (path, arguments, definition): (&str, &[type_ref::Type], _) =
            match &implementation.trait_ {
                Some(item::TraitImpl::Native {
                    path, arguments, ..
                }) => match script(&implementation.owner) {
                    Some(definition) => (path.as_str(), arguments, definition),
                    None => continue,
                },
                Some(item::TraitImpl::User { name, arguments })
                    if name.text == "From"
                        && script(&implementation.owner).is_none()
                        && matches!(
                            &implementation.owner,
                            type_ref::Type::Path(type_ref::Path {
                                target: type_ref::Target::Host { .. },
                                ..
                            })
                        ) =>
                {
                    match arguments.as_ref() {
                        [argument] => match script(argument) {
                            Some(definition) => ("::core::convert::From", arguments, definition),
                            None => continue,
                        },
                        _ => continue,
                    }
                }
                _ => continue,
            };
        // An impl compiled for each use a crate's bound makes of it (D629)
        // is one impl of the runner's per instance, at its types; any other
        // is one, at its own parameters.
        let instances = match program.instances.impls.get(&index) {
            Some(instances) => instances.iter().map(Some).collect::<Vec<_>>(),
            None => vec![None],
        };
        for instance in instances {
            impls.push((
                models.script_path(definition),
                native_impl(
                    program,
                    models,
                    shell,
                    implementation,
                    path,
                    arguments,
                    instance,
                )?,
            ));
        }
    }
    Ok(impls)
}

/// One impl of the runner's for the script's impl `implementation` of the
/// trait at `path` (`native_impls`): at `instance`'s types, or at its own
/// parameters.
#[cfg(feature = "rust-import")]
#[allow(clippy::too_many_lines)]
pub(super) fn native_impl(
    program: &Program,
    models: &Models,
    shell: &item::Shell,
    implementation: &item::Impl,
    path: &str,
    arguments: &[type_ref::Type],
    instance: Option<&crate::resolve::ImplInstance>,
) -> Result<rawr_import::surface::instance::ScriptImpl, crate::resolve::ImportedFailure> {
    use rawr_import::surface::instance::{ImplReceiver, ScriptImpl, ScriptImplFunction};
    {
        let generics = match instance {
            Some(_) => Vec::new(),
            None => implementation
                .generics
                .iter()
                .map(|generic| generic.name.text.clone())
                .collect::<Vec<_>>(),
        };
        let at_instance = |ty: &type_ref::Type| match instance {
            Some(instance) => substitute_parameters(ty, &instance.types),
            None => ty.clone(),
        };
        // An associated type Rust code holds as a script value, as
        // the_matrix's `type State = RefCell<Vec<Cache>>`, has none of the
        // bounds the trait may ask of it, such as `Default`. The runner
        // names it by a proxy of its own beside the owner's, whose
        // `Default` is the type's (`model::associated_path`), and the
        // impl's signatures name it by that proxy where they name the type.
        let owner_path = match &implementation.owner {
            type_ref::Type::Path(type_ref::Path {
                target: type_ref::Target::Script { definition, .. },
                ..
            }) => Some(models.script_path(*definition)),
            _ => None,
        };
        let proxies = implementation
            .associated
            .iter()
            .filter_map(|(name, ty)| {
                let (krate, owner) = owner_path.as_deref()?.split_once("::")?;
                let converted =
                    impl_surface_type(models, &Ty::from_resolved(&at_instance(ty)), &generics)?;
                holds_value(&converted).then(|| {
                    (
                        crate::resolve::full_type_name(ty),
                        rawr_import::classify::representability::convert::Type::Script {
                            path: format!(
                                "{krate}::{}",
                                crate::program::model::associated_path(owner, &name.text)
                            )
                            .into(),
                            arguments: Box::default(),
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        let proxy = |ty: &type_ref::Type| {
            let text = crate::resolve::full_type_name(ty);
            proxies
                .iter()
                .find(|(written, _)| *written == text)
                .map(|(_, proxy)| proxy.clone())
        };
        // A type the impl names that has no Rust type in the runner can't
        // cross, and rustc would refuse the runner without the impl.
        let convert = |ty: &type_ref::Type| {
            if let Some(proxy) = proxy(ty) {
                return Ok(proxy);
            }
            if let type_ref::Type::Reference {
                mutable, target, ..
            } = ty
                && let Some(proxy) = proxy(target)
            {
                return Ok(
                    rawr_import::classify::representability::convert::Type::Borrowed {
                        mutable: *mutable,
                        ty: Box::new(proxy),
                    },
                );
            }
            impl_surface_type(models, &Ty::from_resolved(&at_instance(ty)), &generics).ok_or_else(
                || {
                    crate::resolve::ImportedFailure::Lowering(LoweringFailure::original(
                        shell.source,
                        Error::UnsupportedNominalSyntax {
                            construct: format!(
                                "`{}` in an impl of `{path}` whose functions native code calls \
                             is not supported in Rawr yet; the type has no Rust type for the impl \
                             to name",
                                crate::resolve::full_type_name(ty)
                            )
                            .into(),
                            source: crate::resolve::type_span(ty),
                        }
                        .into(),
                    ))
                },
            )
        };
        let owner = convert(&implementation.owner)?;
        // An instance writes every argument of the trait, its defaults too.
        let arguments = match instance {
            Some(instance) => &instance.arguments[..],
            None => arguments,
        };
        let arguments = arguments
            .iter()
            .map(convert)
            .collect::<Result<Box<[_]>, _>>()?;
        let associated = implementation
            .associated
            .iter()
            .map(|(name, ty)| Ok((Box::<str>::from(name.text.as_str()), convert(ty)?)))
            .collect::<Result<Box<[_]>, crate::resolve::ImportedFailure>>()?;
        let mut functions = Vec::new();
        for function in implementation.functions.iter() {
            let item::Lowering::NominalMethod(id) = function.body.lowering else {
                continue;
            };
            // An impl for a crate's type numbers its functions apart
            // (`resolve::BUILTIN_METHOD_IDS`).
            let definition = match id.index().checked_sub(crate::resolve::BUILTIN_METHOD_IDS) {
                Some(index) => program.builtin_methods.get(index),
                None => program.methods.get(id.index()),
            };
            let Some(definition) = definition else {
                continue;
            };
            // At an instance, the function's instance, whose signature is at
            // the instance's types and whose opaque output is revealed.
            let definition =
                match instance {
                    Some(instance) => {
                        let Some(key) = models.instances.templates.get(definition.key()).and_then(
                            |instances| {
                                instances
                                    .iter()
                                    .find(|(names, _)| *names == instance.names)
                                    .map(|(_, key)| key)
                            },
                        ) else {
                            continue;
                        };
                        let Some(found) = program.methods.iter().find(|method| method.key() == key)
                        else {
                            continue;
                        };
                        found
                    }
                    None => definition,
                };
            // The member as the running program holds it: its owner's path,
            // as the runner names the owner (`crossing_path`), then its name
            // (`crate::island::shared`).
            let owner_path = match definition.key().owner() {
                Some(type_def::Identity::Script(owner)) => models.tables.runtime.crossing_path(
                    &owner
                        .modules()
                        .iter()
                        .map(ImmutableString::as_str)
                        .chain([owner.name()])
                        .collect::<Vec<_>>()
                        .join("::"),
                ),
                _ => continue,
            };
            let receiver = match &function.receiver {
                None => ImplReceiver::None,
                Some(crate::item::Receiver::Shared(_)) => ImplReceiver::Shared,
                Some(crate::item::Receiver::Mutable(_)) => ImplReceiver::Mutable,
                Some(crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_)) => {
                    ImplReceiver::Owned
                }
            };
            let (inputs, output) = match instance {
                Some(_) => (
                    definition
                        .parameters()
                        .iter()
                        .map(|parameter| convert(&parameter.ty))
                        .collect::<Result<Box<[_]>, _>>()?,
                    match &function.output {
                        Some(_) => convert(definition.output())?,
                        None => rawr_import::classify::representability::convert::Type::Unit,
                    },
                ),
                None => (
                    function
                        .parameters
                        .iter()
                        .map(|parameter| convert(&parameter.ty))
                        .collect::<Result<Box<[_]>, _>>()?,
                    match &function.output {
                        Some(output) => convert(output)?,
                        None => rawr_import::classify::representability::convert::Type::Unit,
                    },
                ),
            };
            // Each type parameter native code fills, with its bounds as
            // the impl writes them (#593).
            let mut generics = Vec::with_capacity(function.opaque.len());
            for generic in function.opaque.iter() {
                let mut bounds = Vec::with_capacity(generic.bounds.len());
                for bound in generic.bounds.iter() {
                    bounds.push(rawr_import::surface::instance::ImplBound::new(
                        bound.path.as_str().into(),
                        bound.lifetimes.iter().map(lifetime_text).collect(),
                        bound
                            .arguments
                            .iter()
                            .map(&convert)
                            .collect::<Result<Box<[_]>, _>>()?,
                    ));
                }
                generics.push(rawr_import::surface::instance::ImplGeneric::new(
                    generic.name.as_str().into(),
                    bounds.into_boxed_slice(),
                ));
            }
            functions.push(
                ScriptImplFunction::new(
                    function.name.text.as_str().into(),
                    format!("{owner_path}::{}", definition.key().name()).into(),
                    receiver,
                    inputs,
                    output,
                )
                .with_generics(generics.into_boxed_slice())
                .with_formatter(function.formatter.as_ref().and_then(|formatter| {
                    let position = function
                        .parameters
                        .iter()
                        .position(|parameter| parameter.name.text == *formatter)?;
                    u32::try_from(position).ok()
                })),
            );
        }
        let (lifetimes, lifetime_names) = match &implementation.trait_ {
            Some(item::TraitImpl::Native {
                lifetimes,
                lifetime_names,
                ..
            }) => (*lifetimes, lifetime_names.clone()),
            _ => (0, Box::default()),
        };
        Ok(ScriptImpl::new(
            owner,
            path.into(),
            arguments,
            generics.iter().map(|name| name.as_str().into()).collect(),
            associated,
            functions.into_boxed_slice(),
        )
        .with_lifetimes(u32::try_from(lifetimes).unwrap_or(u32::MAX))
        .with_lifetime_names(lifetime_names.iter().map(lifetime_text).collect()))
    }
}
