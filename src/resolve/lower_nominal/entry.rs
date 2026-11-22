//! The lowering's entry: each body of a program lowered into the runtime's
//! text, with the dispatchers, closures and crate uses it needs.

use super::dispatch::{
    write_closures, write_default_dispatcher, write_function, write_method,
    write_trait_dispatchers, Naming,
};
#[cfg(feature = "rust-import")]
use super::imported::{native_impls, ImportedOutput, Programs};
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
use std::collections::{BTreeMap, BTreeSet};
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
}

impl<'a> Body<'a> {
    /// Where the program writes the body: its source, then its place there.
    pub(super) fn written_at(&self) -> (usize, Position) {
        match self {
            Self::Function(shell, definition) => {
                (shell.source.index(), definition.body.span.start())
            }
            Self::Method(definition) => (definition.source().index(), definition.body().start()),
            Self::Script(script) => (script.source.index(), Position::START),
        }
    }

    /// The source the body is in.
    fn source(&self) -> crate::program::source::Id {
        match self {
            Self::Function(shell, _) => shell.source,
            Self::Method(definition) => definition.source(),
            Self::Script(script) => script.source,
        }
    }

    /// Whether the body is a template's, compiled per instantiation and
    /// lowered as its instances only (D629).
    fn is_template(&self, program: &Program) -> bool {
        let key = match self {
            Self::Function(shell, _) => shell.key.as_ref(),
            Self::Method(definition) => Some(definition.key()),
            Self::Script(_) => None,
        };
        key.is_some_and(|key| program.instances.templates.contains_key(key))
    }

    /// The text the body's lowering reads.
    pub(super) fn text(&self, program: &Program) -> std::borrow::Cow<'a, str> {
        match self {
            Self::Function(shell, definition) => body_source(
                definition
                    .body
                    .text
                    .as_deref()
                    .unwrap_or_else(|| program.source.text_at(shell.source)),
                &definition.body,
            )
            .into(),
            Self::Method(definition) => source_span(
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
            Self::Function(_, definition) => {
                let _ = write!(
                    written,
                    "f{:?}|{:?}|",
                    definition.body.span, definition.body.elide
                );
                if let Some(text) = &definition.body.text {
                    super::interface::framed(&mut written.0, text);
                }
            }
            Self::Method(definition) => {
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
        self.hidden
            .push((body.source(), std::mem::take(&mut lowered.hidden)));
        match body {
            Body::Function(shell, definition) => {
                let generated_name = function_name(
                    shell
                        .key
                        .as_ref()
                        .expect("resolved functions retain stable item keys"),
                );
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
            Body::Method(definition) => {
                if definition.receiver().is_some() {
                    self.receivers
                        .insert(method_name(definition).into(), "self".into());
                }
                let offset = write_method(&mut self.generated, definition, &lowered, unit.models);
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
                self.declare(method_name(definition).into(), lowered.declared);
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
        Body::Function(shell, definition) => {
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

    let mut dispatch_ops = Vec::new();
    write_trait_dispatchers(&mut assembly.generated, &models, &mut dispatch_ops);
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
    witnessed_hidden(&witnesses, &assembly.hidden, &models)?;
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
    // Each async body becomes a state machine, once its names are final.
    let mut unsupported = None;
    ast.for_each_fn_def_mut(|function| {
        if unsupported.is_none() {
            unsupported =
                crate::eval::machine::lower(function, crate::eval::machine::Frames::Resumed).err();
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
}

/// What a body knows of opaque outputs' hidden types (D1109): the hidden
/// type it fills each of its function's with, and each opaque output a
/// generic call in it takes as a type argument, at the call, whose witness
/// is its hidden type's once every body is checked.
#[derive(Clone, Default)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Hidden {
    pub(super) filled: Vec<(type_ref::Opaque, Ty)>,
    pub(super) witnessed: Vec<Witnessed>,
    pub(super) sends: Vec<HiddenSend>,
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

/// The witness of an opaque output's one hidden type (D1109): known, or
/// the witness of the type argument it is, at each call.
#[derive(Clone, PartialEq)]
enum HiddenWitness {
    Known(String),
    Captured(usize),
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
/// one hidden type has one.
fn write_hidden_dispatcher(
    output: &mut location::Generated,
    hidden: &[(crate::program::source::Id, Hidden)],
    models: &Models,
) -> BTreeMap<type_ref::Opaque, Option<HiddenWitness>> {
    let mut witnesses = BTreeMap::<type_ref::Opaque, Option<HiddenWitness>>::new();
    for (opaque, ty) in hidden.iter().flat_map(|(_, hidden)| &hidden.filled) {
        let witness = match ty {
            Ty::Script(id, _) => Some(HiddenWitness::Known(id.number().to_string())),
            // `fn wrap<T>(t: T) -> impl Shape { t }`: its call's `T`'s.
            Ty::Param(name, super::ty::Parameter::Declared) => models
                .opaque_outputs
                .get(opaque)
                .and_then(|output| output.captured.iter().position(|captured| captured == name))
                .map(HiddenWitness::Captured),
            ty => witness_code(ty).map(|code| HiddenWitness::Known(code.to_string())),
        };
        // A trait method's opaque output, which each impl fills (stage 4),
        // has no one hidden type yet.
        witnesses
            .entry(*opaque)
            .and_modify(|known| {
                if *known != witness {
                    *known = None;
                }
            })
            .or_insert(witness);
    }
    let mut text = String::new();
    let mut at = None;
    for (opaque, witness) in &witnesses {
        let Some(witness) = witness else {
            continue;
        };
        let [source, line, column, index] = opaque.numbers();
        let witness = match witness {
            HiddenWitness::Known(witness) => witness.clone(),
            HiddenWitness::Captured(at) => format!("__rawr_arguments[{at}]"),
        };
        let _ = writeln!(
            text,
            "if __rawr_source == {source} && __rawr_line == {line} \
             && __rawr_column == {column} && __rawr_index == {index} {{ return {witness}; }}"
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

/// Refuse a generic call taking as a type argument an opaque output with
/// no one hidden type a witness names, or whose hidden type is a type
/// argument with none at the call, as it refuses that type argument.
fn witnessed_hidden(
    witnesses: &BTreeMap<type_ref::Opaque, Option<HiddenWitness>>,
    hidden: &[(crate::program::source::Id, Hidden)],
    models: &Models,
) -> Result<(), LoweringFailure> {
    for (source, hidden) in hidden {
        for Witnessed {
            opaque,
            span,
            arguments,
        } in &hidden.witnessed
        {
            let witnessed = match witnesses.get(opaque) {
                Some(Some(HiddenWitness::Known(_))) => true,
                Some(Some(HiddenWitness::Captured(at))) => arguments.get(*at) == Some(&true),
                _ => false,
            };
            if witnessed {
                continue;
            }
            let name = models.opaque_outputs.get(opaque).map_or_else(
                || "impl Trait".into(),
                |output| output.bounds.name.text.clone(),
            );
            return Err(LoweringFailure::original(
                *source,
                Error::UnsupportedNominalSyntax {
                    construct: format!("`{name}` as the type argument of a generic call").into(),
                    source: *span,
                }
                .into(),
            ));
        }
    }
    Ok(())
}

/// Refuse a thread's closure capturing an opaque output whose hidden type
/// isn't `Send`, as rustc's auto traits see through it (D1109). An opaque
/// output with no one hidden type passes, as rawr proves no auto trait.
fn sent_hidden(
    engine: &Engine,
    hidden: &[(crate::program::source::Id, Hidden)],
    models: &Models,
) -> Result<(), LoweringFailure> {
    let mut revealed = BTreeMap::<type_ref::Opaque, Option<Ty>>::new();
    for (opaque, ty) in hidden.iter().flat_map(|(_, hidden)| &hidden.filled) {
        revealed
            .entry(*opaque)
            .and_modify(|known| {
                if known.as_ref() != Some(ty) {
                    *known = None;
                }
            })
            .or_insert_with(|| Some(ty.clone()));
    }
    // At its type arguments: `wrap::<Rc<f64>>`'s hidden `T` is `Rc<f64>`.
    let reveal = |ty: &Ty| match ty {
        Ty::Param(_, super::ty::Parameter::Opaque(captured)) => {
            let hidden = revealed.get(&captured.opaque)?.as_ref()?;
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
