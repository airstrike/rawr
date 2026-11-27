//! The lowering's entry: each body of a program lowered into the runtime's
//! text, with the dispatchers, closures and crate uses it needs.

use super::dispatch::{
    write_closures, write_default_dispatcher, write_function, write_method,
    write_trait_dispatchers, Naming,
};
use super::imported::named_type;
#[cfg(feature = "rust-import")]
use super::imported::{native_impls, ImportedOutput, Programs};
use super::instance::{Bound, Demand, Uses};
use super::location::{resolve_locations, retain_locations, Location};
use super::models::Models;
use super::naming::{
    function_name, method_name, pattern_name, private_function_name, private_method_name,
    trait_dispatcher_name, DEBUG_DISPATCHER, DEBUG_GENERIC, HIDDEN_DISPATCHER, NATIVE_ONLY,
    SENTINEL,
};
use super::op::{register, witness_code, ExecutableOp, PreludeOp};
use super::ty::Ty;
use super::{
    closure, lex_rust, location, rewrite_statements, source_full_span, source_span, this_parameter,
    Parser,
};
#[cfg(feature = "rust-import")]
use super::{kept, splice, ImportedContext};
use crate::ast::NominalPattern;
use crate::resolve::{item, type_ref, Error, LoweringFailure, Program};
use crate::tokenizer::Token;
use crate::{method, Engine, ImmutableString, Module, Position, Scope, Shared, Span, AST};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

pub(in crate::resolve) fn lower(
    engine: &Engine,
    program: &Program,
    runtime_model: &crate::program::model::Model,
    scope: Option<&Scope>,
    optimization_level: crate::OptimizationLevel,
) -> Result<Output, LoweringFailure> {
    let models = Models::new(
        program,
        runtime_model.clone(),
        engine.host_registry.share(),
        engine,
    );
    lower_with_context(
        engine,
        program,
        models,
        scope,
        optimization_level,
        #[cfg(feature = "rust-import")]
        None,
        #[cfg(feature = "rust-import")]
        None,
    )
}

/// Lower executable support and retain its exact imported uses in the same pass.
#[cfg(feature = "rust-import")]
pub(crate) fn with_uses<A: crate::host::ImportAllocation>(
    engine: &Engine,
    kept: Option<&mut kept::Lowerings>,
    program: &Program,
    runtime_model: &crate::program::model::Model,
    allocation: &mut A,
    scope: Option<&Scope>,
    optimization_level: crate::OptimizationLevel,
) -> Result<(Output, ImportedOutput), crate::resolve::ImportedFailure> {
    let mut models = Models::new(
        program,
        runtime_model.clone(),
        engine.host_registry.share(),
        engine,
    );
    models._imported_owner = allocation.support_owner();
    let mut uses = Vec::new();
    let mut dependencies = BTreeSet::new();
    let mut allocation_failure = None;
    // What a part lent no session reads, which nothing keeps.
    let mut reads = Vec::new();
    let programs = Programs::of(program);
    let impls = native_impls(program, &models, &programs)?.into_boxed_slice();
    let lowered = lower_with_context(
        engine,
        program,
        models,
        scope,
        optimization_level,
        kept,
        Some(ImportedContext {
            native: program.native.as_deref(),
            allocation,
            failure: &mut allocation_failure,
            source: program.source.entry(),
            uses: &mut uses,
            dependencies: &mut dependencies,
            reads: &mut reads,
        }),
    );
    if let Some(error) = allocation_failure {
        return Err(crate::resolve::ImportedFailure::Surface(error));
    }
    let lowered = lowered.map_err(crate::resolve::ImportedFailure::Lowering)?;
    uses.sort_unstable();
    uses.dedup();
    Ok((
        lowered,
        ImportedOutput {
            uses: uses.into_boxed_slice(),
            impls,
            programs,
        },
    ))
}

/// A function's or a method's body, or the root's statements outside its
/// items, which `lower_with_context` lowers.
#[derive(Clone, Copy)]
pub(super) enum Body<'a> {
    Function(&'a item::Shell, &'a item::Function),
    Method(&'a method::Definition),
    Script(&'a crate::resolve::Script),
    /// A template's instance (D1115): the template's function, in its
    /// source, read at the instance's types.
    Instance(&'a item::Shell, &'a item::Function, &'a Demand, &'a [Bound]),
    /// A template method's instance, read at the instance's types.
    MethodInstance(&'a method::Definition, &'a Demand, &'a [Bound]),
}

impl<'a> Body<'a> {
    /// Where the program writes the body: its source, then its place there.
    pub(super) fn written_at(&self) -> (usize, Position) {
        match self {
            Self::Function(shell, definition) | Self::Instance(shell, definition, ..) => {
                (shell.source.index(), definition.body.span.start())
            }
            Self::Method(definition) | Self::MethodInstance(definition, ..) => {
                (definition.source().index(), definition.body().start())
            }
            Self::Script(script) => (script.source.index(), Position::START),
        }
    }

    /// The source the body is in.
    fn source(&self) -> crate::program::source::Id {
        match self {
            Self::Function(shell, _) | Self::Instance(shell, ..) => shell.source,
            Self::Method(definition) | Self::MethodInstance(definition, ..) => definition.source(),
            Self::Script(script) => script.source,
        }
    }

    /// Whether the body is a template's, compiled per instantiation and
    /// lowered as its instances only (D629).
    fn is_template(&self, program: &Program) -> bool {
        let key = match self {
            Self::Function(shell, _) => shell.key.as_ref(),
            Self::Method(definition) => Some(definition.key()),
            Self::Script(_) | Self::Instance(..) | Self::MethodInstance(..) => None,
        };
        key.is_some_and(|key| program.instances.templates.contains_key(key))
    }

    /// The template whose generic read this body is (D1115).
    fn template(&self, models: &Models) -> Option<crate::item::Key> {
        match self {
            Self::Function(shell, _) => shell
                .key
                .clone()
                .filter(|key| models.templates.contains_key(key)),
            Self::Method(definition) => {
                Some(definition.key().clone()).filter(|key| models.templates.contains_key(key))
            }
            Self::Script(_) | Self::Instance(..) | Self::MethodInstance(..) => None,
        }
    }

    /// The text the body's lowering reads.
    pub(super) fn text(&self, program: &Program) -> std::borrow::Cow<'a, str> {
        match self {
            Self::Function(shell, definition) | Self::Instance(shell, definition, ..) => {
                body_source(
                    definition
                        .body
                        .text
                        .as_deref()
                        .unwrap_or_else(|| program.source.text_at(shell.source)),
                    &definition.body,
                )
                .into()
            }
            Self::Method(definition) | Self::MethodInstance(definition, ..) => source_span(
                definition
                    .body_text()
                    .unwrap_or_else(|| program.source.text_at(definition.source())),
                definition.body(),
            )
            .into(),
            Self::Script(script) => script.text.as_str().into(),
        }
    }

    /// What its text holds beside its source's, which a key that writes
    /// this writes too: where the body is, what of it is elided, and the
    /// text Rawr wrote for it. A key never builds the text (#1850).
    #[cfg(feature = "rust-import")]
    pub(super) fn write_key(&self, written: &mut super::interface::Written) {
        use std::fmt::Write as _;
        // A span's `Debug` can't fail to write.
        match self {
            Self::Function(_, definition) | Self::Instance(_, definition, ..) => {
                let _ = write!(
                    written,
                    "f{:?}|{:?}|",
                    definition.body.span, definition.body.elide
                );
                if let Some(text) = &definition.body.text {
                    super::interface::framed(&mut written.0, text);
                }
            }
            Self::Method(definition) | Self::MethodInstance(definition, ..) => {
                let _ = write!(written, "m{:?}|", definition.body());
                if let Some(text) = definition.body_text() {
                    super::interface::framed(&mut written.0, text);
                }
            }
            Self::Script(script) => super::interface::framed(&mut written.0, &script.text),
        }
    }
}

/// What the lowering writes as it goes: the generated text, and what each
/// body adds to it.
struct Assembly {
    generated: location::Generated,
    locations: Vec<location::Pending>,
    /// Each body's ops and patterns, by the names its text calls them by.
    ops: Vec<(ImmutableString, ExecutableOp)>,
    patterns: Vec<(ImmutableString, NominalPattern)>,
    closures: Vec<closure::Callable>,
    symbol_rewrites: BTreeMap<ImmutableString, ImmutableString>,
    /// The name the program gave each generated function's receiver, which
    /// its body reads as `this`: a debugger reads it by that name (#534).
    receivers: BTreeMap<ImmutableString, ImmutableString>,
    /// Where each free function's `&mut` parameter, its receiver, is
    /// written among its parameters, for a host call that passes it there.
    written_receivers: BTreeMap<ImmutableString, usize>,
    /// Each generated function's locals a debugger prints by their type
    /// (#512).
    declared: BTreeMap<ImmutableString, Vec<Declared>>,
    /// What each body, in its source, knows of opaque outputs' hidden
    /// types (D1109).
    hidden: Vec<(crate::program::source::Id, Hidden)>,
    /// What each body found of instances (D1115), with the template whose
    /// generic read it is.
    instances: Vec<(Option<crate::item::Key>, Uses)>,
    /// The instances lowered so far, by key.
    instanced: BTreeSet<crate::item::Key>,
    /// Each program method whose body (an instance's or its own) makes a
    /// crate call that runs only in a lending region, with that call's
    /// item (#2698).
    lending: BTreeMap<crate::item::Key, ImmutableString>,
}

/// One source's part being lowered, and what its lowering reads.
struct Unit<'a> {
    engine: &'a Engine,
    program: &'a Program,
    models: &'a Models,
    scope: Option<&'a Scope<'a>>,
    source: crate::program::source::Id,
    /// What its ops' and patterns' names say before their numbers
    /// (`Parser::numbering`).
    numbering: ImmutableString,
}

/// How many ops, patterns and closures a part's bodies lowered so far,
/// which number the next body's.
struct Counts<'a> {
    ops: usize,
    patterns: usize,
    closures: Naming<'a>,
}

impl<'a> Counts<'a> {
    /// Nothing lowered yet of `unit`'s part.
    fn new(unit: &'a Unit<'_>) -> Self {
        Self {
            ops: 0,
            patterns: 0,
            closures: Naming::new(&unit.numbering),
        }
    }
}

impl Assembly {
    /// Keep the locals of the generated function `name` a debugger prints
    /// by their type.
    fn declare(&mut self, name: ImmutableString, declared: Vec<Declared>) {
        if !declared.is_empty() {
            self.declared.insert(name, declared);
        }
    }

    /// Write `lowered`, the lowering of `body`, into the generated text.
    fn place(
        &mut self,
        unit: &Unit<'_>,
        counts: &mut Counts<'_>,
        body: Body<'_>,
        mut lowered: Lowered,
    ) {
        // An opaque output's hidden type is its template's generic read's,
        // in the template's parameters, which each call's types fill.
        if let Body::Instance(..) = body {
            lowered.hidden.filled.clear();
        }
        self.hidden
            .push((body.source(), std::mem::take(&mut lowered.hidden)));
        let uses = std::mem::take(&mut lowered.instances);
        if let (Some(item), Body::Method(definition) | Body::MethodInstance(definition, ..)) =
            (&uses.lending, &body)
        {
            self.lending
                .entry(definition.key().clone())
                .or_insert_with(|| item.clone());
        }
        self.instances.push((body.template(unit.models), uses));
        match body {
            Body::Function(shell, definition) | Body::Instance(shell, definition, ..) => {
                let instance = match body {
                    Body::Instance(_, _, demand, _) => Some(demand.key(unit.models)),
                    _ => None,
                };
                let generated_name = function_name(instance.as_ref().unwrap_or_else(|| {
                    shell
                        .key
                        .as_ref()
                        .expect("resolved functions retain stable item keys")
                }));
                if let Some(instance) = &instance {
                    self.symbol_rewrites.insert(
                        generated_name.as_str().into(),
                        if unit.program.source.is_aggregate() {
                            private_function_name(instance)
                        } else {
                            instance.name().into()
                        },
                    );
                }
                if let Some(index) = this_parameter(definition) {
                    self.receivers.insert(
                        generated_name.as_str().into(),
                        definition.parameters[index].name.text.clone(),
                    );
                    self.written_receivers
                        .insert(generated_name.as_str().into(), index);
                }
                let offset = write_function(
                    &mut self.generated,
                    shell.source,
                    &generated_name,
                    definition,
                    &lowered,
                    unit.models,
                );
                retain_locations(
                    &mut self.locations,
                    offset,
                    Some(
                        shell
                            .key
                            .as_ref()
                            .expect("resolved functions retain stable item keys"),
                    ),
                    shell.source,
                    lowered.locations,
                );
                self.number(unit, counts, lowered.ops, lowered.patterns);
                write_closures(
                    &mut self.generated,
                    &mut self.locations,
                    shell.source,
                    &mut counts.closures,
                    lowered.closures,
                    &mut self.closures,
                    &mut self.symbol_rewrites,
                );
                self.declare(generated_name.as_str().into(), lowered.declared);
            }
            Body::Method(definition) | Body::MethodInstance(definition, ..) => {
                let name = match body {
                    Body::MethodInstance(_, demand, _) => {
                        let instance = demand.key(unit.models);
                        let name = demand.name(unit.models);
                        self.symbol_rewrites
                            .insert(name.as_str().into(), method::symbol(&instance));
                        name
                    }
                    _ => method_name(definition),
                };
                if definition.receiver().is_some() {
                    self.receivers.insert(name.as_str().into(), "self".into());
                }
                let offset = write_method(
                    &mut self.generated,
                    &name,
                    definition,
                    &lowered,
                    unit.models,
                );
                retain_locations(
                    &mut self.locations,
                    offset,
                    Some(definition.key()),
                    definition.source(),
                    lowered.locations,
                );
                self.number(unit, counts, lowered.ops, lowered.patterns);
                write_closures(
                    &mut self.generated,
                    &mut self.locations,
                    definition.source(),
                    &mut counts.closures,
                    lowered.closures,
                    &mut self.closures,
                    &mut self.symbol_rewrites,
                );
                self.declare(name.into(), lowered.declared);
            }
            Body::Script(script) => {
                let offset = self.generated.body(
                    script.source,
                    source_full_span(unit.program.source.text_at(script.source)),
                    &lowered.source,
                    &lowered.locations,
                );
                // The script's statements: a runtime error in them is
                // reported at the script's own text.
                retain_locations(
                    &mut self.locations,
                    offset,
                    None,
                    script.source,
                    lowered.locations,
                );
                self.number(unit, counts, lowered.ops, lowered.patterns);
            }
        }
    }

    /// Name a body's ops and patterns as its text calls them.
    fn number(
        &mut self,
        unit: &Unit<'_>,
        counts: &mut Counts<'_>,
        ops: Vec<ExecutableOp>,
        patterns: Vec<NominalPattern>,
    ) {
        for op in ops {
            let name = format!("{SENTINEL}{}{}", unit.numbering, counts.ops);
            self.ops.push((name.into(), op));
            counts.ops += 1;
        }
        for pattern in patterns {
            let name = pattern_name(&unit.numbering, counts.patterns);
            self.patterns.push((name.into(), pattern));
            counts.patterns += 1;
        }
    }
}

/// Lower `bodies`, a source's part, into `assembly`. A runner's compiler
/// reuses what it lowered of the part before when nothing its lowering
/// reads changed, and charges again what it charged then (#1646).
fn lower_part(
    unit: &Unit<'_>,
    assembly: &mut Assembly,
    bodies: &[Body<'_>],
    #[cfg(feature = "rust-import")] imported: Option<&mut ImportedContext<'_>>,
    #[cfg(feature = "rust-import")] session: Option<(&mut kept::Session<'_>, kept::Part)>,
) -> Result<(), LoweringFailure> {
    let mut counts = Counts::new(unit);
    #[cfg(feature = "rust-import")]
    if let Some(imported) = imported {
        let mut session = session;
        let key = session
            .as_ref()
            .and_then(|(session, part)| session.key(*part, unit.source, unit.program, bodies));
        if let (Some((session, _)), Some(key)) = (session.as_mut(), key)
            && let Some(reused) = session.reuse(key, unit.models)
            && let Some(renewed) = reused
                .lowered
                .iter()
                .map(|lowered| lowered.renewed(unit.models))
                .collect::<Option<Vec<_>>>()
        {
            for &(items, bytes) in &reused.outside.charges {
                imported
                    .retain_checked(items, bytes)
                    .map_err(|error| LoweringFailure::original(unit.source, error.into()))?;
            }
            imported.uses.extend(reused.uses.iter().cloned());
            imported
                .dependencies
                .extend(reused.dependencies.iter().cloned());
            for (body, lowered) in bodies.iter().zip(renewed) {
                assembly.place(unit, &mut counts, *body, lowered);
            }
            return Ok(());
        }
        let mut recording = kept::Recording {
            inner: &mut *imported.allocation,
            charges: Vec::new(),
        };
        let mut uses = Vec::new();
        let mut dependencies = BTreeSet::new();
        let mut lowerings = Vec::new();
        // What this part reads outside the program is kept with it.
        let mut reads = Vec::new();
        {
            let mut context = ImportedContext {
                native: imported.native,
                allocation: &mut recording,
                failure: &mut *imported.failure,
                source: unit.source,
                uses: &mut uses,
                dependencies: &mut dependencies,
                reads: &mut reads,
            };
            for body in bodies {
                let text = body.text(unit.program);
                let lowered = lower_body(
                    unit,
                    &counts,
                    *body,
                    &text,
                    Some(context.for_source(unit.source)),
                )?;
                if key.is_some() {
                    lowerings.push(lowered.clone());
                }
                assembly.place(unit, &mut counts, *body, lowered);
            }
        }
        if let Some((session, _)) = session {
            session.keep(
                unit.source,
                key,
                &lowerings,
                &uses,
                &dependencies,
                kept::Outside::new(recording.charges, reads),
            );
        }
        imported.uses.append(&mut uses);
        imported.dependencies.append(&mut dependencies);
        return Ok(());
    }
    for body in bodies {
        let text = body.text(unit.program);
        let lowered = lower_body(
            unit,
            &counts,
            *body,
            &text,
            #[cfg(feature = "rust-import")]
            None,
        )?;
        assembly.place(unit, &mut counts, *body, lowered);
    }
    Ok(())
}

/// Lower one body from `text`, its ops and patterns numbered after
/// `counts`.
// The parser reads `text` as long as it holds `imported`, which only a
// rust-import build has.
#[cfg_attr(not(feature = "rust-import"), allow(clippy::needless_lifetimes))]
fn lower_body<'a>(
    unit: &Unit<'_>,
    counts: &Counts<'_>,
    body: Body<'_>,
    text: &'a str,
    #[cfg(feature = "rust-import")] imported: Option<ImportedContext<'a>>,
) -> Result<Lowered, LoweringFailure> {
    let models = unit.models.for_source(unit.source);
    match body {
        Body::Instance(shell, definition, demand, bound) => {
            let parameters = models
                .templates
                .get(&demand.template)
                .cloned()
                .expect("an instance's template is one of the program's templates");
            let key = demand.key(&models);
            let types = demand
                .types
                .iter()
                .map(|ty| {
                    named_type(ty, &models, definition.name.span)
                        .expect("an instance's types are each one a signature names")
                })
                .collect::<Vec<_>>();
            let instance = super::instance::definition(definition, &parameters, &types);
            let mut parser = Parser::for_function(
                unit.engine,
                text,
                models,
                &instance,
                key,
                counts.ops,
                counts.patterns,
                #[cfg(feature = "rust-import")]
                imported,
            );
            parser.numbering = unit.numbering.clone();
            parser.enter_instance(
                (&definition.parameters, &definition.generics),
                &parameters,
                &demand.types,
            );
            let source = unit.program.source.text_at(shell.source);
            parser.qualify_bound_methods(unit.engine, &definition.generics, bound, source);
            parser.qualify_parameter_paths(unit.engine, &definition.generics, &parameters, source);
            parser
                .parse_function_body(&instance)
                .map_err(|error| LoweringFailure::original(shell.source, error.into()))
        }
        Body::MethodInstance(definition, demand, bound) => {
            let parameters = models
                .templates
                .get(&demand.template)
                .cloned()
                .expect("an instance's template is one of the program's templates");
            let substituted = parameters
                .iter()
                .cloned()
                .zip(demand.types.iter().map(|ty| {
                    named_type(ty, &models, definition.name_span())
                        .expect("an instance's types are each one a signature names")
                }))
                .collect::<Vec<_>>();
            let key = demand.key(&models);
            let instance =
                definition.instance(definition.id(), key.clone(), key.name().into(), |ty| {
                    type_ref::substitute_parameters(ty, &substituted)
                });
            let mut parser = Parser::for_method(
                unit.engine,
                text,
                models,
                &instance,
                counts.ops,
                counts.patterns,
                #[cfg(feature = "rust-import")]
                imported,
            )
            .map_err(|error| LoweringFailure::original(unit.source, error.into()))?;
            parser.numbering = unit.numbering.clone();
            parser.enter_instance(
                (definition.parameters(), definition.generics()),
                &parameters,
                &demand.types,
            );
            let source = unit.program.source.text_at(definition.source());
            parser.qualify_bound_methods(unit.engine, definition.generics(), bound, source);
            parser.qualify_parameter_paths(unit.engine, definition.generics(), &parameters, source);
            parser
                .parse_method_body(&instance)
                .map_err(|error| LoweringFailure::original(unit.source, error.into()))
        }
        Body::Function(shell, definition) => {
            let template = body.template(&models).is_some();
            let mut parser = Parser::for_function(
                unit.engine,
                text,
                models,
                definition,
                shell
                    .key
                    .clone()
                    .expect("resolved functions retain stable item keys"),
                counts.ops,
                counts.patterns,
                #[cfg(feature = "rust-import")]
                imported,
            );
            parser.numbering = unit.numbering.clone();
            parser.template_read = template;
            parser
                .parse_function_body(definition)
                .map_err(|error| LoweringFailure::original(unit.source, error.into()))
        }
        Body::Method(definition) => {
            let mut parser = Parser::for_method(
                unit.engine,
                text,
                models,
                definition,
                counts.ops,
                counts.patterns,
                #[cfg(feature = "rust-import")]
                imported,
            )
            .map_err(|error| LoweringFailure::original(unit.source, error.into()))?;
            parser.numbering = unit.numbering.clone();
            parser.template_read = body.template(unit.models).is_some();
            parser
                .parse_method_body(definition)
                .map_err(|error| LoweringFailure::original(unit.source, error.into()))
        }
        Body::Script(_) => {
            let mut parser = Parser::new_with_offset(
                unit.engine,
                text,
                models,
                counts.ops,
                counts.patterns,
                #[cfg(feature = "rust-import")]
                imported,
            );
            parser.numbering = unit.numbering.clone();
            if let Some(scope) = unit.scope {
                parser.bind_scope(scope);
            }
            parser
                .parse()
                .map_err(|error| LoweringFailure::original(unit.source, error.into()))
        }
    }
}

/// Lower each instance the program's bodies bind (D1115), and each one an
/// instance binds in turn, once each, in its template's source. A template
/// whose erased body a call still needs, from a body that runs, must have
/// one: else that call reports what its generic read can't lower.
fn lower_instances(
    unit: &Unit<'_>,
    assembly: &mut Assembly,
    after_script: Option<crate::program::source::Id>,
    #[cfg(feature = "rust-import")] mut imported: Option<&mut ImportedContext<'_>>,
) -> Result<(), LoweringFailure> {
    let shells = unit
        .program
        .items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Function(definition) => Some((shell.key.clone()?, (shell, definition))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut queue = VecDeque::new();
    demands(assembly, 0, 0, &mut queue);
    if let Some(source) = after_script
        && !queue.is_empty()
    {
        assembly.generated.declaration(source, Span::NONE, ";\n");
    }
    while let Some((demand, depth)) = queue.pop_front() {
        let key = demand.key(unit.models);
        if !assembly.instanced.insert(key.clone()) {
            continue;
        }
        let method = unit
            .program
            .methods
            .iter()
            .chain(&unit.program.builtin_methods)
            .find(|definition| demand.method() && definition.key() == &demand.template);
        let function = shells.get(&demand.template).copied();
        let source = match (method, function) {
            (Some(definition), _) => definition.source(),
            (None, Some((shell, _))) => shell.source,
            (None, None) => continue,
        };
        if depth >= unit.models.macros.limit() {
            return Err(LoweringFailure::original(
                source,
                Error::Syntax {
                    message: format!(
                        "reached the recursion limit while instantiating `{}`",
                        key.name()
                    )
                    .into(),
                    label: ImmutableString::new(),
                    help: None,
                    at: demand.call,
                }
                .into(),
            ));
        }
        let bound = assembly
            .instances
            .iter()
            .find(|(template, _)| template.as_ref() == Some(&demand.template))
            .map(|(_, uses)| uses.bound.clone())
            .unwrap_or_default();
        let from = assembly.instances.len();
        let body = match (method, function) {
            (Some(definition), _) => Body::MethodInstance(definition, &demand, &bound),
            (None, Some((shell, definition))) => Body::Instance(shell, definition, &demand, &bound),
            (None, None) => unreachable!("an instance's template is a function or a method"),
        };
        lower_part(
            &Unit {
                engine: unit.engine,
                program: unit.program,
                models: unit.models,
                scope: unit.scope,
                source,
                numbering: format!("{}n{}_", source.index(), assembly.instanced.len()).into(),
            },
            assembly,
            &[body],
            #[cfg(feature = "rust-import")]
            imported.as_deref_mut(),
            #[cfg(feature = "rust-import")]
            None,
        )?;
        demands(assembly, from, depth + 1, &mut queue);
    }
    Ok(())
}

/// Whether each template whose erased body a running body calls has one:
/// else that call reports what its generic read can't lower (D1115), or
/// the failure `gaps` keeps for it.
fn erased_needed(
    unit: &Unit<'_>,
    assembly: &Assembly,
    gaps: &mut BTreeMap<crate::item::Key, LoweringFailure>,
) -> Result<(), LoweringFailure> {
    // The erased bodies that run: those the running bodies call, and
    // those each of those calls.
    let own = assembly
        .instances
        .iter()
        .filter_map(|(template, uses)| Some((template.clone()?, uses)))
        .collect::<BTreeMap<_, _>>();
    let mut needed = assembly
        .instances
        .iter()
        .filter(|(template, _)| template.is_none())
        .flat_map(|(_, uses)| uses.erased.iter().cloned())
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some((template, _)) = needed.pop() {
        if !seen.insert(template.clone()) {
            continue;
        }
        let Some(uses) = own.get(&template) else {
            continue;
        };
        if !uses.gap {
            needed.extend(uses.erased.iter().cloned());
            continue;
        }
        if let Some(failure) = gaps.remove(&template) {
            return Err(failure);
        }
        if let Some(shell) = unit
            .program
            .items
            .iter()
            .find(|shell| shell.key.as_ref() == Some(&template))
            && let item::Kind::Function(definition) = &shell.kind
        {
            return Err(erased_failure(unit, Body::Function(shell, definition)));
        }
        if let Some(definition) = unit
            .program
            .methods
            .iter()
            .chain(&unit.program.builtin_methods)
            .find(|definition| definition.key() == &template)
        {
            return Err(erased_failure(unit, Body::Method(definition)));
        }
    }
    Ok(())
}

/// Whether no body calls a program method that runs only where native
/// code calls it, outside a method of the same trait (#2698): its body
/// makes a crate call, as iced's `overlay`, whose output keeps what native
/// code lends the program's method in a lending region, and a program's
/// own call opens none, so the call is refused where the program lowers,
/// as a program's own call of the crate's item is (`refuse_unlent_call`).
/// The check refuses it for a body it lowers (`imported`); a template's
/// instance lowers only here, when the program starts.
fn lending_called(assembly: &Assembly) -> Result<(), LoweringFailure> {
    let calls =
        assembly
            .instances
            .iter()
            .zip(&assembly.hidden)
            .flat_map(|((_, uses), (source, _))| {
                uses.method_calls
                    .iter()
                    .map(|(callee, call)| (*source, callee, *call))
            });
    refuse_lending_calls(&assembly.lending, calls)
}

/// The first of `calls`, each a source, the program method it calls and
/// the call, whose method is one of `lending`'s, refused with the crate
/// call its body makes (#2698).
pub(super) fn refuse_lending_calls<'k>(
    lending: &BTreeMap<crate::item::Key, ImmutableString>,
    mut calls: impl Iterator<Item = (crate::program::source::Id, &'k crate::item::Key, Span)>,
) -> Result<(), LoweringFailure> {
    if lending.is_empty() {
        return Ok(());
    }
    let Some((source, callee, call, item)) = calls.find_map(|(source, callee, call)| {
        lending.get(callee).map(|item| (source, callee, call, item))
    }) else {
        return Ok(());
    };
    Err(LoweringFailure::original(
        source,
        Error::InvalidHostOperation {
            item: callee.name().into(),
            reason: format!(
                "its body calls `{item}`, whose output keeps its `&mut` receiver and a `&mut` \
                 input borrowed, which Rawr lends only where native code calls a program's \
                 method of this shape, as iced asks a widget for its `overlay`: calling it \
                 from the program isn't supported yet"
            )
            .into(),
            operation: call,
        }
        .into(),
    ))
}

/// Move the demands of the bodies `assembly` placed from `from` on into
/// `queue`, each `depth` instances deep.
fn demands(
    assembly: &mut Assembly,
    from: usize,
    depth: usize,
    queue: &mut VecDeque<(Demand, usize)>,
) {
    for (_, uses) in &mut assembly.instances[from..] {
        queue.extend(
            std::mem::take(&mut uses.demands)
                .into_iter()
                .map(|demand| (demand, depth)),
        );
    }
}

/// Check the default body `definition` no impl receives, as the method of
/// its trait's check-only `impl<Self: Trait> Trait for Self` (#191): what
/// rustc's borrow checker reports in it, read with `Self` a parameter the
/// trait bounds. Its lowering never runs. Any other report is left to
/// `cargo check`, as before: a generic read rawr can't make there yet is
/// no verdict of rustc's.
fn check_default(
    engine: &Engine,
    program: &Program,
    models: &Models,
    definition: &method::Definition,
) -> Result<(), LoweringFailure> {
    let text = Body::Method(definition).text(program);
    let source = definition.source();
    let read = Parser::for_method(
        engine,
        &text,
        models.for_source(source),
        definition,
        0,
        0,
        #[cfg(feature = "rust-import")]
        None,
    )
    .and_then(|mut parser| parser.parse_method_body(definition));
    match read {
        Err(error)
            if matches!(
                error.category(),
                crate::resolve::Category::BorrowConflict
                    | crate::resolve::Category::MovedValue
                    | crate::resolve::Category::ImmutableBinding
            ) =>
        {
            Err(LoweringFailure::original(source, error.into()))
        }
        Ok(_) => Ok(()),
        Err(_) => {
            #[cfg(test)]
            crate::resolve::borrowck::note_unlowered(
                "a default body no impl receives that rawr's generic read stops in",
                definition.body(),
            );
            Ok(())
        }
    }
}

/// What the erased body of a template, `body`, reports, read as a body
/// that runs: what its generic read could lower only per instance.
fn erased_failure(unit: &Unit<'_>, body: Body<'_>) -> LoweringFailure {
    let text = body.text(unit.program);
    let source = body.source();
    let models = unit.models.for_source(source);
    let read = match body {
        Body::Function(shell, definition) => Parser::for_function(
            unit.engine,
            &text,
            models,
            definition,
            shell
                .key
                .clone()
                .expect("resolved functions retain stable item keys"),
            0,
            0,
            #[cfg(feature = "rust-import")]
            None,
        )
        .parse_function_body(definition),
        Body::Method(definition) => Parser::for_method(
            unit.engine,
            &text,
            models,
            definition,
            0,
            0,
            #[cfg(feature = "rust-import")]
            None,
        )
        .and_then(|mut parser| parser.parse_method_body(definition)),
        Body::Script(_) | Body::Instance(..) | Body::MethodInstance(..) => {
            unreachable!("a template is a function or a method")
        }
    };
    let Err(error) = read else {
        unreachable!("a body whose generic read has a gap fails as an erased body");
    };
    LoweringFailure::original(source, error.into())
}

pub(super) fn lower_with_context(
    engine: &Engine,
    program: &Program,
    models: Models,
    scope: Option<&Scope>,
    optimization_level: crate::OptimizationLevel,
    #[cfg(feature = "rust-import")] kept: Option<&mut kept::Lowerings>,
    #[cfg(feature = "rust-import")] mut imported: Option<ImportedContext<'_>>,
) -> Result<Output, LoweringFailure> {
    let mut symbol_rewrites: BTreeMap<ImmutableString, ImmutableString> = program
        .methods
        .iter()
        .chain(&program.builtin_methods)
        .map(|definition| {
            (
                method_name(definition).into(),
                private_method_name(definition).into(),
            )
        })
        .collect();
    // A script's functions keep their names, for the host to call. The
    // lowered text calls them by a sentinel, so a name the lowering writes
    // for std (`abs(x)` of `x.abs()`, `v.push(x)`) is never one of them,
    // and that name reaches only the runtime's own function, as rustc
    // resolves a method before any free function.
    let aggregate = program.source.is_aggregate();
    symbol_rewrites.extend(program.items.iter().flat_map(|shell| {
        let item::Kind::Function(function) = &shell.kind else {
            return Vec::new();
        };
        let key = shell
            .key
            .as_ref()
            .expect("resolved functions retain stable item keys");
        if aggregate {
            let private = (function_name(key).into(), private_function_name(key));
            // A root function a single file's program also names by its own
            // name (`Executable::into_one_file`) is never a std call's.
            if key.modules().is_empty() && key.owner().is_none() {
                return vec![private, (function.name.text.clone(), NATIVE_ONLY.into())];
            }
            return vec![private];
        }
        vec![
            (function_name(key).into(), function.name.text.clone()),
            (function.name.text.clone(), NATIVE_ONLY.into()),
        ]
    }));
    // `type Output: Clone;`: each impl's associated type has its trait's
    // bounds, its owner the trait's supertraits, and its trait arguments
    // the trait's bounds on its parameters, as Rust checks at the impl.
    for shell in program.items.iter() {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        let supertraits = match &implementation.trait_ {
            Some(item::TraitImpl::User { name, .. }) => {
                models.traits.get(&name.text).is_some_and(|(_, declared)| {
                    !declared.supertraits.1.is_empty()
                        || declared.supertraits.0.iter().next().is_some()
                        || declared.generics.iter().any(|generic| {
                            generic.bounds.iter().next().is_some() || !generic.traits.is_empty()
                        })
                })
            }
            _ => false,
        };
        if implementation.associated.is_empty() && !supertraits {
            continue;
        }
        let mut parser = Parser::new_with_offset(
            engine,
            "",
            models.for_source(shell.source),
            0,
            0,
            #[cfg(feature = "rust-import")]
            None,
        );
        parser
            .check_associated_bounds(implementation)
            .map_err(|error| LoweringFailure::original(shell.source, error.into()))?;
    }

    // Bodies are checked in the order the program writes them, so the
    // first error reported is the first rustc reports.
    let mut bodies = program
        .items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Function(definition) => Some(Body::Function(shell, definition)),
            _ => None,
        })
        .chain(
            program
                .methods
                .iter()
                .chain(&program.builtin_methods)
                .map(Body::Method),
        )
        .collect::<Vec<_>>();
    bodies.sort_by_key(Body::written_at);
    let mut assembly = Assembly {
        generated: location::Generated::new(),
        locations: Vec::new(),
        ops: Vec::new(),
        patterns: Vec::new(),
        closures: Vec::new(),
        symbol_rewrites,
        receivers: BTreeMap::new(),
        written_receivers: BTreeMap::new(),
        declared: BTreeMap::new(),
        hidden: Vec::new(),
        instances: Vec::new(),
        instanced: BTreeSet::new(),
        lending: BTreeMap::new(),
    };
    // A compile lent a runner's lowerings lowers again only the sources
    // whose lowering reads something that changed (#1646).
    #[cfg(feature = "rust-import")]
    let mut session = kept
        .filter(|_| imported.is_some())
        .map(|kept| kept::Session::open(engine, kept, program, &models, scope, optimization_level));
    // Where each part's text is: a runner's compile parses again only the
    // parts whose text changed (#1877).
    #[cfg(feature = "rust-import")]
    let mut pieces = splice::Pieces::default();
    // The default bodies no impl receives, checked at their traits (#191):
    // each a source's bodies written after it don't precede.
    let mut checks = program.checked_defaults.iter().collect::<Vec<_>>();
    checks.sort_by_key(|definition| Body::Method(definition).written_at());
    let mut checks = checks.into_iter().peekable();
    // A source's bodies are lowered together, their ops and patterns
    // numbered after the source.
    for group in bodies.chunk_by(|body, next| body.source() == next.source()) {
        let lowerable = group
            .iter()
            .filter(|body| !body.is_template(program))
            .copied()
            .collect::<Vec<_>>();
        let Some(first) = lowerable.first() else {
            continue;
        };
        let source = first.source();
        let start = first.written_at();
        while let Some(definition) =
            checks.next_if(|definition| Body::Method(definition).written_at() < start)
        {
            check_default(engine, program, &models, definition)?;
        }
        lower_part(
            &Unit {
                engine,
                program,
                models: &models,
                scope,
                source,
                numbering: format!("{}_", source.index()).into(),
            },
            &mut assembly,
            &lowerable,
            #[cfg(feature = "rust-import")]
            imported.as_mut(),
            #[cfg(feature = "rust-import")]
            session
                .as_mut()
                .map(|session| (session, kept::Part::Bodies)),
        )?;
        #[cfg(feature = "rust-import")]
        pieces.written(assembly.generated.len(), Some((source, kept::Part::Bodies)));
    }

    for definition in checks {
        check_default(engine, program, &models, definition)?;
    }

    if let Some(first) = bodies.first() {
        lower_instances(
            &Unit {
                engine,
                program,
                models: &models,
                scope,
                source: first.source(),
                numbering: ImmutableString::new(),
            },
            &mut assembly,
            None,
            #[cfg(feature = "rust-import")]
            imported.as_mut(),
        )?;
    }

    // The arms of the dispatchers the bodies' trait objects reach
    // (D1115.3). The root's statements come after the dispatchers, and a
    // Rust program's run them only to call `main`.
    let arms = assembly
        .instances
        .iter()
        .flat_map(|(_, uses)| uses.objects.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut dispatch_ops = Vec::new();
    write_trait_dispatchers(&mut assembly.generated, &models, &mut dispatch_ops, &arms);
    write_default_dispatcher(&mut assembly.generated, program, &models, &mut dispatch_ops);
    let witnesses = write_hidden_dispatcher(&mut assembly.generated, &assembly.hidden, &models);
    #[cfg(feature = "rust-import")]
    pieces.written(assembly.generated.len(), None);

    let script = program
        .scripts
        .iter()
        .find(|script| script.source == program.source.entry())
        .expect("resolved single-source programs retain their root script");
    lower_part(
        &Unit {
            engine,
            program,
            models: &models,
            scope,
            source: script.source,
            numbering: format!("{}s", script.source.index()).into(),
        },
        &mut assembly,
        &[Body::Script(script)],
        #[cfg(feature = "rust-import")]
        imported.as_mut(),
        #[cfg(feature = "rust-import")]
        session
            .as_mut()
            .map(|session| (session, kept::Part::Script)),
    )?;
    #[cfg(feature = "rust-import")]
    pieces.written(
        assembly.generated.len(),
        Some((script.source, kept::Part::Script)),
    );
    // The instances the script's statements bind come after them, which
    // its value ends.
    lower_instances(
        &Unit {
            engine,
            program,
            models: &models,
            scope,
            source: script.source,
            numbering: ImmutableString::new(),
        },
        &mut assembly,
        Some(script.source),
        #[cfg(feature = "rust-import")]
        imported.as_mut(),
    )?;
    let mut gaps = witnessed_hidden(&witnesses, &mut assembly, &models)?;
    erased_needed(
        &Unit {
            engine,
            program,
            models: &models,
            scope,
            source: script.source,
            numbering: ImmutableString::new(),
        },
        &assembly,
        &mut gaps,
    )?;
    lending_called(&assembly)?;
    sent_hidden(engine, &assembly.hidden, &models)?;
    #[cfg(feature = "rust-import")]
    if let Some(session) = &mut session {
        session.succeeded();
    }
    let Assembly {
        generated,
        locations,
        ops,
        patterns,
        closures,
        symbol_rewrites,
        receivers,
        written_receivers,
        declared,
        hidden: _,
        instances: _,
        instanced: _,
        lending: _,
    } = assembly;
    // The dispatchers' ops are numbered apart from every source's.
    let ops = dispatch_ops
        .into_iter()
        .enumerate()
        .map(|(index, op)| (ImmutableString::from(format!("{SENTINEL}{index}")), op))
        .chain(ops)
        .collect::<Vec<_>>();

    #[cfg(feature = "rust-import")]
    let spliced = match &mut session {
        Some(session) => pieces.parse(engine, scope, &generated, session, optimization_level),
        None => Ok(None),
    };
    #[cfg(not(feature = "rust-import"))]
    let spliced = Ok(None);
    let parsed = spliced.and_then(|spliced| match spliced {
        Some(ast) => Ok(ast),
        None => engine.compile_legacy_script_with_scope_raw(
            scope,
            generated.as_str(),
            optimization_level,
        ),
    });
    let mut ast = match parsed {
        Ok(ast) => ast,
        Err(error) => {
            return Err(match generated.translate(error.primary_span()) {
                Ok(translated) => {
                    LoweringFailure::generated(translated.source, translated.span, error)
                }
                Err(origin) => LoweringFailure::unmapped(
                    format!("cannot retain compiler diagnostic origin: {origin}"),
                    error,
                ),
            });
        }
    };
    let mut module = Module::new();
    let mut rewrites = symbol_rewrites;
    let support_id = models.support_id();
    let models = Shared::new(models);
    let dependencies = ops
        .iter()
        .flat_map(|(_, op)| op.dependencies())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let runtime_requirements = ops
        .iter()
        .filter_map(|(_, op)| op.runtime_requirement())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let projection_requirements = ops
        .iter()
        .filter_map(|(_, op)| op.projection_requirement())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let nominal_patterns: BTreeMap<ImmutableString, NominalPattern> =
        patterns.into_iter().collect();

    for (sentinel, op) in ops {
        let private: ImmutableString = op.private_name(support_id).into();
        rewrites.insert(sentinel, private.clone());
        register(&mut module, private, op, &models);
    }
    // A debugger reads any value's `Debug` text through it (#488). Every
    // program carries it, so a debugger registered while the program runs
    // reads what one registered from its start does (#2297). Its name is
    // private to the compiler: a program's own `{:?}` is checked as Rust
    // checks it.
    debug_assert_eq!(
        format!("\0rawr.{}", PreludeOp::GenericDebug.name()),
        DEBUG_GENERIC
    );
    debug_assert_eq!(trait_dispatcher_name("Debug", "fmt"), DEBUG_DISPATCHER);
    #[cfg(feature = "debugging")]
    {
        register(
            &mut module,
            DEBUG_GENERIC.into(),
            ExecutableOp::Prelude(PreludeOp::GenericDebug),
            &models,
        );
        // Its locals by their functions' names as the program runs them.
        let declared = declared
            .into_iter()
            .map(|(name, locals)| {
                let name = rewrites
                    .get(&name)
                    .filter(|renamed| renamed.as_str() != NATIVE_ONLY)
                    .cloned()
                    .unwrap_or(name);
                (name, locals)
            })
            .collect();
        super::format::register_declared_debug(&mut module, &models, declared);
    }
    #[cfg(not(feature = "debugging"))]
    drop((declared, receivers));

    rewrite_statements(ast.statements_mut(), &rewrites, &nominal_patterns);
    ast.for_each_fn_def_mut(|function| {
        rewrite_statements(function.body.statements_mut(), &rewrites, &nominal_patterns);
        // The receiver's name, which a debugger reads it by.
        #[cfg(feature = "debugging")]
        {
            function.this_name = receivers.get(&function.name).cloned();
        }
        function.this_parameter = written_receivers.get(&function.name).copied();
        if let Some(name) = rewrites.get(&function.name)
            && name.as_str() != NATIVE_ONLY
        {
            function.name = name.clone();
        }
    });
    // Each async body becomes a state machine, once its names are final,
    // with its frames as its closure's are kept.
    let copied = closures
        .iter()
        .filter(|closure| matches!(closure.frames, crate::eval::machine::Frames::Copied))
        .map(|closure| closure.dispatch.clone())
        .collect::<BTreeSet<_>>();
    let mut unsupported = None;
    ast.for_each_fn_def_mut(|function| {
        if unsupported.is_none() {
            let frames = if copied.contains(&function.name) {
                crate::eval::machine::Frames::Copied
            } else {
                crate::eval::machine::Frames::Resumed
            };
            unsupported = crate::eval::machine::lower(function, frames).err();
        }
    });
    if let Some(crate::eval::machine::Unsupported(what, position)) = unsupported {
        let error = crate::ParseError(
            Box::new(crate::ParseErrorType::BadInput(
                crate::LexError::ImproperSymbol(
                    "await".into(),
                    format!("{what} is not supported in Rawr yet"),
                ),
            )),
            position,
            crate::types::parse_error::Rendered::default(),
        );
        return Err(match generated.translate(Span::new(position, position)) {
            Ok(translated) => LoweringFailure::generated(translated.source, translated.span, error),
            Err(origin) => LoweringFailure::unmapped(
                format!("cannot retain compiler diagnostic origin: {origin}"),
                error,
            ),
        });
    }
    ast.reindex_script_functions();
    ast.add_support(module);
    let locations = resolve_locations(generated.as_str(), locations);
    Ok(Output {
        ast,
        locations: locations.into_boxed_slice(),
        dependencies,
        runtime_requirements,
        projection_requirements,
        closures: closures.into_boxed_slice(),
    })
}

pub(crate) struct Output {
    pub(crate) ast: AST,
    pub(crate) locations: Box<[Location]>,
    pub(crate) dependencies: Box<[crate::host::facade::Dependency]>,
    pub(crate) runtime_requirements: Box<[crate::host::runtime::Requirement]>,
    pub(crate) projection_requirements: Box<[crate::host::projection::Site]>,
    pub(crate) closures: Box<[closure::Callable]>,
}

/// The path of each trait object type the program's sources write, by its
/// segments: `["std", "fmt", "Debug"]` of `dyn std::fmt::Debug`. A source
/// with no `dyn` is never lexed.
pub(super) fn trait_objects(engine: &Engine, source: &crate::program::Source) -> Vec<Vec<String>> {
    let mut objects = Vec::new();
    for text in source.texts().filter(|text| text.contains("dyn")) {
        let tokens = lex_rust(engine, text);
        let mut rest = tokens.iter().map(|(token, _)| token).peekable();
        while let Some(token) = rest.next() {
            if !matches!(token, Token::Identifier(name) | Token::Reserved(name)
                if name.as_str() == "dyn")
            {
                continue;
            }
            let mut segments = Vec::new();
            while let Some(Token::Identifier(name) | Token::Reserved(name)) = rest.peek() {
                segments.push(name.to_string());
                rest.next();
                if rest
                    .next_if(|token| matches!(token, Token::DoubleColon))
                    .is_none()
                {
                    break;
                }
            }
            if !segments.is_empty() {
                objects.push(segments);
            }
        }
    }
    objects
}

#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Lowered {
    pub(super) source: String,
    pub(super) locations: Vec<location::Output>,
    pub(super) ops: Vec<ExecutableOp>,
    pub(super) patterns: Vec<NominalPattern>,
    pub(super) closures: Vec<closure::Lifted>,
    /// The body's locals a debugger prints by their type (#512).
    pub(super) declared: Vec<Declared>,
    pub(super) hidden: Hidden,
    /// The instances its calls bind (D1115).
    pub(super) instances: super::instance::Uses,
}

/// What a body knows of opaque outputs' hidden types (D1109): the hidden
/// type it fills each of its function's with, and each opaque output a
/// generic call in it takes as a type argument, at the call, whose witness
/// is its hidden type's once every body is checked.
#[derive(Clone, Default)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Hidden {
    pub(super) filled: Vec<Filled>,
    pub(super) witnessed: Vec<Witnessed>,
    pub(super) sends: Vec<HiddenSend>,
}

/// The hidden type a body fills an opaque output with, and the type
/// arguments the output has in that body: a trait's provided method,
/// copied for each impl that keeps it, fills its trait's one opaque output
/// at each impl's `Self` (D1109), as rustc's `<A as Make>::make` is its own
/// type for each `A`.
#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Filled {
    pub(super) opaque: type_ref::Opaque,
    pub(super) hidden: Ty,
    pub(super) arguments: Box<[Ty]>,
}

/// An opaque output a generic call takes as a type argument at `span`,
/// and which of its own type arguments have a witness there: the one a
/// hidden type that is one of them passes (D1109).
#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Witnessed {
    pub(super) opaque: type_ref::Opaque,
    pub(super) span: Span,
    pub(super) arguments: Box<[bool]>,
}

/// The witness of an opaque output's hidden type (D1109): known, the
/// witness of the type argument it is at each call, or known for each list
/// of its type arguments' witnesses, which a call's own select.
#[derive(Clone, PartialEq)]
enum HiddenWitness {
    Known(String),
    Captured(usize),
    ByArguments(Vec<(Box<[String]>, String)>),
}

/// A `Send` bound of `item` on a type `ty` that holds an opaque output,
/// needed at `at`, which holds as the hidden types do (D1109): a thread's
/// closure capturing it, whose head is `closure`, a generic call's
/// argument, or what a thread gives.
#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct HiddenSend {
    pub(super) ty: Ty,
    pub(super) at: Span,
    pub(super) closure: Option<Span>,
    pub(super) item: Option<ImmutableString>,
}

/// A declaration of a local whose `Debug` text a debugger reads by its
/// type, not its value alone (`format::needs_declared`): its name, the
/// line it is declared on, and its type; `None` for another declaration of
/// the name, which prints as its value does.
#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Declared {
    pub(super) name: String,
    // Read by a debugger only, and kept the same without one, so a lowering
    // kept on disk serves both.
    #[cfg_attr(not(feature = "debugging"), allow(dead_code))]
    pub(super) line: usize,
    pub(super) ty: Option<super::ty::Ty>,
}

/// The function from each opaque output, and the witnesses of its type
/// arguments, to the witness of its hidden type (D1109), which a generic
/// call taking it as a type argument passes: written once every
/// function's body has filled its own. Each opaque output's witness, if
/// its hidden types have one.
fn write_hidden_dispatcher(
    output: &mut location::Generated,
    hidden: &[(crate::program::source::Id, Hidden)],
    models: &Models,
) -> BTreeMap<type_ref::Opaque, Option<HiddenWitness>> {
    let mut fills = BTreeMap::<type_ref::Opaque, Vec<&Filled>>::new();
    for filled in hidden.iter().flat_map(|(_, hidden)| &hidden.filled) {
        fills.entry(filled.opaque).or_default().push(filled);
    }
    let witnesses = fills
        .into_iter()
        .map(|(opaque, fills)| (opaque, hidden_witness(opaque, &fills, models)))
        .collect::<BTreeMap<_, _>>();
    let mut text = String::new();
    let mut at = None;
    for (opaque, witness) in &witnesses {
        let Some(witness) = witness else {
            continue;
        };
        let [source, line, column, index] = opaque.numbers();
        let body = match witness {
            HiddenWitness::Known(witness) => format!("return {witness};"),
            HiddenWitness::Captured(at) => format!("return __rawr_arguments[{at}];"),
            HiddenWitness::ByArguments(arms) => {
                let mut body = String::new();
                for (arguments, witness) in arms {
                    let test = arguments
                        .iter()
                        .enumerate()
                        .map(|(at, argument)| format!("__rawr_arguments[{at}] == {argument}"))
                        .collect::<Vec<_>>()
                        .join(" && ");
                    let _ = write!(body, "if {test} {{ return {witness}; }} ");
                }
                body
            }
        };
        let _ = writeln!(
            text,
            "if __rawr_source == {source} && __rawr_line == {line} \
             && __rawr_column == {column} && __rawr_index == {index} {{ {body}}}"
        );
        at.get_or_insert(opaque.function());
    }
    if let Some((source, span)) = at {
        output.declaration(
            source,
            span,
            &format!(
                "__rawr_private fn {HIDDEN_DISPATCHER}(__rawr_source, __rawr_line, \
                 __rawr_column, __rawr_index, __rawr_arguments) {{\n{text}}}\n"
            ),
        );
    }
    witnesses
}

/// The witness of `opaque`'s hidden type, which `fills` fill it with: the
/// one every fill gives, else the one each list of type arguments the fills
/// have gives, as each impl keeping a trait's provided method fills the
/// method's opaque output at its own `Self`. `None` when it has none.
fn hidden_witness(
    opaque: type_ref::Opaque,
    fills: &[&Filled],
    models: &Models,
) -> Option<HiddenWitness> {
    let known = |ty: &Ty| match ty {
        Ty::Script(id, _) => Some(id.number().to_string()),
        ty => witness_code(ty).map(|code| code.to_string()),
    };
    let witness = |filled: &Filled| match &filled.hidden {
        // `fn wrap<T>(t: T) -> impl Shape { t }`: its call's `T`'s.
        Ty::Param(name, super::ty::Parameter::Declared) => models
            .opaque_outputs
            .get(&opaque)
            .and_then(|output| output.captured.iter().position(|captured| captured == name))
            .map(HiddenWitness::Captured),
        ty => known(ty).map(HiddenWitness::Known),
    };
    let first = witness(fills.first()?);
    if fills.iter().all(|filled| witness(filled) == first) {
        return first;
    }
    // A trait's opaque output, which each impl keeping the provided method
    // fills at its own type arguments.
    let mut arms = Vec::<(Box<[String]>, String)>::new();
    for filled in fills {
        let arguments = filled
            .arguments
            .iter()
            .map(known)
            .collect::<Option<Box<[_]>>>()?;
        let hidden = known(&filled.hidden)?;
        match arms.iter().find(|(known, _)| *known == arguments) {
            Some((_, other)) if *other != hidden => return None,
            Some(_) => {}
            None => arms.push((arguments, hidden)),
        }
    }
    Some(HiddenWitness::ByArguments(arms))
}

/// Refuse a generic call taking as a type argument an opaque output with
/// no one hidden type a witness names, or whose hidden type is a type
/// argument with none at the call, as it refuses that type argument. A
/// template's generic read may not know the hidden type its parameters
/// choose, as `<M as Make>::make`'s is each impl's, which each instance
/// names (D1115): its erased body then runs only where a call needs it,
/// which reports this, by the template's key.
fn witnessed_hidden(
    witnesses: &BTreeMap<type_ref::Opaque, Option<HiddenWitness>>,
    assembly: &mut Assembly,
    models: &Models,
) -> Result<BTreeMap<crate::item::Key, LoweringFailure>, LoweringFailure> {
    let mut gaps = BTreeMap::new();
    for ((source, hidden), (template, uses)) in assembly.hidden.iter().zip(&mut assembly.instances)
    {
        for Witnessed {
            opaque,
            span,
            arguments,
        } in &hidden.witnessed
        {
            let witnessed = match witnesses.get(opaque) {
                Some(Some(HiddenWitness::Known(_))) => true,
                Some(Some(HiddenWitness::Captured(at))) => arguments.get(*at) == Some(&true),
                Some(Some(HiddenWitness::ByArguments(_))) => arguments.iter().all(|known| *known),
                _ => false,
            };
            if witnessed {
                continue;
            }
            let name = models.opaque_outputs.get(opaque).map_or_else(
                || "impl Trait".into(),
                |output| output.bounds.name.text.clone(),
            );
            let failure = LoweringFailure::original(
                *source,
                Error::UnsupportedNominalSyntax {
                    construct: format!("`{name}` as the type argument of a generic call").into(),
                    source: *span,
                }
                .into(),
            );
            match template {
                Some(template) => {
                    uses.gap = true;
                    gaps.entry(template.clone()).or_insert(failure);
                }
                None => return Err(failure),
            }
        }
    }
    Ok(gaps)
}

/// Refuse a thread's closure capturing an opaque output whose hidden type
/// isn't `Send`, as rustc's auto traits see through it (D1109). An opaque
/// output with no one hidden type at its type arguments passes, as rawr
/// proves no auto trait.
fn sent_hidden(
    engine: &Engine,
    hidden: &[(crate::program::source::Id, Hidden)],
    models: &Models,
) -> Result<(), LoweringFailure> {
    let mut fills = BTreeMap::<type_ref::Opaque, Vec<&Filled>>::new();
    for filled in hidden.iter().flat_map(|(_, hidden)| &hidden.filled) {
        fills.entry(filled.opaque).or_default().push(filled);
    }
    // At its type arguments: `wrap::<Rc<f64>>`'s hidden `T` is `Rc<f64>`,
    // and a trait's provided method's is the one its impl's `Self` fills.
    let reveal = |ty: &Ty| match ty {
        Ty::Param(_, super::ty::Parameter::Opaque(captured)) => {
            let fills = fills.get(&captured.opaque)?;
            let first = &fills.first()?.hidden;
            let hidden = if fills.iter().all(|filled| filled.hidden == *first) {
                first
            } else {
                &fills
                    .iter()
                    .find(|filled| *filled.arguments == *captured.arguments)?
                    .hidden
            };
            let output = models.opaque_outputs.get(&captured.opaque)?;
            let substitution = super::generics::opaque_substitution(output, &captured.arguments);
            Some((
                super::generics::instantiate(hidden, &substitution),
                output.bounds.name.span,
            ))
        }
        _ => None,
    };
    for (source, hidden) in hidden {
        if hidden.sends.is_empty() {
            continue;
        }
        let parser = Parser::new_with_offset(
            engine,
            "",
            models.for_source(*source),
            0,
            0,
            #[cfg(feature = "rust-import")]
            None,
        );
        for send in &hidden.sends {
            let Some(unsendable) = parser.unsendable(&send.ty, &reveal) else {
                continue;
            };
            let unsendable = match send.closure {
                Some(head) => unsendable.in_closure(head),
                None => unsendable,
            };
            let required_by = send.item.clone().map(crate::resolve::Required::by);
            return Err(LoweringFailure::original(
                *source,
                unsendable.error(models, send.at, required_by).into(),
            ));
        }
    }
    Ok(())
}

/// The blocks of the import at `span` among a source's `imports`, and of
/// each import there that binds what it binds, which hoisting merged into
/// it: `None` when one of them is the module's.
pub(super) fn use_blocks(imports: &[crate::item::Use], span: Span) -> Option<Vec<Span>> {
    let key = imports.iter().find(|used| used.span == span)?.hoist_key();
    imports
        .iter()
        .filter(|used| used.hoist_key() == key)
        .map(|used| used.scope)
        .collect()
}

/// Each name of an imported enum variant (`use Shape::*;`) as its path,
/// `Shape::Circle`, where a path may start: not after `.` or `::`, and not
/// before a `:` that names a field or a `::` that goes on to a name. A
/// turbofish (`Empty::<i32>`) stays on the variant.
pub(super) fn expand_imported_variants(
    models: &Models,
    tokens: Vec<(Token, Position)>,
) -> Vec<(Token, Position)> {
    let mut expanded = Vec::with_capacity(tokens.len());
    for index in 0..tokens.len() {
        let (token, position) = &tokens[index];
        let name = match token {
            Token::Identifier(name)
                if !matches!(
                    expanded.last(),
                    Some((Token::Period | Token::DoubleColon, _))
                ) && !matches!(
                    (tokens.get(index + 1), tokens.get(index + 2)),
                    (Some((Token::Colon, _)), _)
                        | (
                            Some((Token::DoubleColon, _)),
                            Some((Token::Identifier(_), _))
                        )
                ) =>
            {
                Some(name)
            }
            _ => None,
        };
        // A crate enum's variant is its enum's, `Alignment::Center`.
        if let Some(path) = name.and_then(|name| models.imported_host_variant(name, *position)) {
            for (segment, name) in path.iter().enumerate() {
                if segment > 0 {
                    expanded.push((Token::DoubleColon, *position));
                }
                expanded.push((Token::Identifier(Box::new(name.as_str().into())), *position));
            }
            continue;
        }
        let owner = name.and_then(|name| models.imported_variant(name, *position));
        for segment in owner.into_iter().flatten() {
            expanded.push((
                Token::Identifier(Box::new(segment.as_str().into())),
                *position,
            ));
            expanded.push((Token::DoubleColon, *position));
        }
        expanded.push((token.clone(), *position));
    }
    expanded
}

/// A function's body as `source_span` selects it, with its elided source
/// blank.
pub(super) fn body_source(source: &str, body: &item::Body) -> String {
    let selected = source_span(source, body.span);
    let Some(elide) = body.elide else {
        return selected;
    };
    let mut output = String::with_capacity(selected.len());
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in selected.chars() {
        if character == '\n' {
            output.push(character);
            line = line.saturating_add(1);
            column = 0;
            continue;
        }
        column = column.saturating_add(1);
        let position = Position::new(line, column);
        output.push(if position >= elide.start() && position < elide.end() {
            ' '
        } else {
            character
        });
    }
    output
}

/// Whether `path` names a runtime's entry macro: a crate's `main`, as
/// `#[tokio::main]` and `#[async_std::main]` are.
pub(in crate::resolve) fn runtime_entry(path: &crate::item::type_ref::Path) -> bool {
    path.segments.len() > 1
        && path
            .segments
            .last()
            .is_some_and(|segment| segment.name.text == "main")
}
