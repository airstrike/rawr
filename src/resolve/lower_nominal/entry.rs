//! The lowering's entry: each body of a program lowered into the runtime's
//! text, with the dispatchers, closures and crate uses it needs.

use super::dispatch::{
    write_closures, write_default_dispatcher, write_function, write_method, write_trait_dispatchers,
};
#[cfg(feature = "rust-import")]
use super::imported::{native_impls, ImportedOutput};
use super::location::{resolve_locations, retain_locations, Location};
use super::models::Models;
use super::naming::{
    function_name, method_name, pattern_name, private_function_name, private_method_name,
    trait_dispatcher_name, DEBUG_DISPATCHER, DEBUG_GENERIC, NATIVE_ONLY, SENTINEL,
};
use super::op::{register, ExecutableOp, PreludeOp};
#[cfg(feature = "rust-import")]
use super::ImportedContext;
use super::{
    body_source, closure, lex_rust, location, rewrite_statements, source_full_span, source_span,
    this_parameter, Parser,
};
use crate::ast::NominalPattern;
use crate::resolve::{item, LoweringFailure, Program};
use crate::tokenizer::Token;
use crate::{method, Engine, ImmutableString, Module, Position, Scope, Shared, Span, AST};
use std::collections::{BTreeMap, BTreeSet};

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
    )
}

/// Lower executable support and retain its exact imported uses in the same pass.
#[cfg(feature = "rust-import")]
pub(crate) fn with_uses<A: crate::host::ImportAllocation>(
    engine: &Engine,
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
    let impls = native_impls(program, &models)?.into_boxed_slice();
    let lowered = lower_with_context(
        engine,
        program,
        models,
        scope,
        optimization_level,
        Some(ImportedContext {
            native: program.native.as_deref(),
            allocation,
            failure: &mut allocation_failure,
            source: program.source.entry(),
            uses: &mut uses,
            dependencies: &mut dependencies,
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
        },
    ))
}

/// A function's or a method's body, which `lower_with_context` lowers.
#[derive(Clone, Copy)]
pub(super) enum Body<'a> {
    Function(&'a item::Shell, &'a item::Function),
    Method(&'a method::Definition),
}

impl Body<'_> {
    /// Where the program writes the body: its source, then its place there.
    pub(super) fn written_at(&self) -> (usize, Position) {
        match self {
            Self::Function(shell, definition) => {
                (shell.source.index(), definition.body.span.start())
            }
            Self::Method(definition) => (definition.source().index(), definition.body().start()),
        }
    }
}

pub(super) fn lower_with_context(
    engine: &Engine,
    program: &Program,
    models: Models,
    scope: Option<&Scope>,
    optimization_level: crate::OptimizationLevel,
    #[cfg(feature = "rust-import")] mut imported: Option<ImportedContext<'_>>,
) -> Result<Output, LoweringFailure> {
    // The name the program gave each generated function's receiver, which
    // its body reads as `this`: a debugger reads it by that name (#534).
    let mut receivers: BTreeMap<ImmutableString, ImmutableString> = BTreeMap::new();
    // Where each free function's `&mut` parameter, its receiver, is written
    // among its parameters, for a host call that passes it there.
    let mut written_receivers: BTreeMap<ImmutableString, usize> = BTreeMap::new();
    let mut symbol_rewrites: BTreeMap<ImmutableString, ImmutableString> = program
        .methods
        .iter()
        .chain(&program.builtin_methods)
        .map(|definition| {
            (
                method_name(definition.id()).into(),
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
        if aggregate {
            let key = shell
                .key
                .as_ref()
                .expect("resolved functions retain stable item keys");
            let private = (function_name(shell.id).into(), private_function_name(key));
            // A root function a single file's program also names by its own
            // name (`Executable::into_one_file`) is never a std call's.
            if key.modules().is_empty() && key.owner().is_none() {
                return vec![private, (function.name.text.clone(), NATIVE_ONLY.into())];
            }
            return vec![private];
        }
        vec![
            (function_name(shell.id).into(), function.name.text.clone()),
            (function.name.text.clone(), NATIVE_ONLY.into()),
        ]
    }));
    let mut generated = location::Generated::new();
    let mut closures = Vec::new();
    let mut locations = Vec::new();
    let mut ops = Vec::new();
    let mut patterns = Vec::new();

    // `type Output: Clone;`: each impl's associated type has its trait's
    // bounds, and its owner the trait's supertraits, as Rust checks at the
    // impl.
    for shell in program.items.iter() {
        let item::Kind::Impl(implementation) = &shell.kind else {
            continue;
        };
        let supertraits = match &implementation.trait_ {
            Some(item::TraitImpl::User { name, .. }) => {
                models.traits.get(&name.text).is_some_and(|(_, declared)| {
                    !declared.supertraits.1.is_empty()
                        || declared.supertraits.0.iter().next().is_some()
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
    for body in bodies {
        match body {
            Body::Function(shell, definition) => {
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
                let mut parser = Parser::for_function(
                    engine,
                    &body_source,
                    models.for_source(shell.source),
                    definition,
                    shell
                        .key
                        .clone()
                        .expect("resolved functions retain stable item keys"),
                    ops.len(),
                    patterns.len(),
                    #[cfg(feature = "rust-import")]
                    imported
                        .as_mut()
                        .map(|context| context.for_source(shell.source)),
                );
                let lowered = parser
                    .parse_function_body(definition)
                    .map_err(|error| LoweringFailure::original(shell.source, error.into()))?;
                let generated_name = function_name(shell.id);
                if let Some(index) = this_parameter(definition) {
                    receivers.insert(
                        generated_name.as_str().into(),
                        definition.parameters[index].name.text.clone(),
                    );
                    written_receivers.insert(generated_name.as_str().into(), index);
                }
                let body = write_function(
                    &mut generated,
                    shell.source,
                    &generated_name,
                    definition,
                    &lowered,
                    &models,
                );
                retain_locations(
                    &mut locations,
                    body,
                    Some(
                        shell
                            .key
                            .as_ref()
                            .expect("resolved functions retain stable item keys"),
                    ),
                    shell.source,
                    lowered.locations,
                );
                ops.extend(lowered.ops);
                patterns.extend(lowered.patterns);
                write_closures(
                    &mut generated,
                    &mut locations,
                    shell.source,
                    lowered.closures,
                    &mut closures,
                    &mut symbol_rewrites,
                );
            }
            Body::Method(definition) => {
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
                let mut parser = Parser::for_method(
                    engine,
                    &body_source,
                    models.for_source(definition.source()),
                    definition,
                    ops.len(),
                    patterns.len(),
                    #[cfg(feature = "rust-import")]
                    imported
                        .as_mut()
                        .map(|context| context.for_source(definition.source())),
                )
                .map_err(|error| LoweringFailure::original(definition.source(), error.into()))?;
                let lowered = parser.parse_method_body(definition).map_err(|error| {
                    LoweringFailure::original(definition.source(), error.into())
                })?;
                if definition.receiver().is_some() {
                    receivers.insert(method_name(definition.id()).into(), "self".into());
                }
                let body = write_method(&mut generated, definition, &lowered, &models);
                retain_locations(
                    &mut locations,
                    body,
                    Some(definition.key()),
                    definition.source(),
                    lowered.locations,
                );
                ops.extend(lowered.ops);
                patterns.extend(lowered.patterns);
                write_closures(
                    &mut generated,
                    &mut locations,
                    definition.source(),
                    lowered.closures,
                    &mut closures,
                    &mut symbol_rewrites,
                );
            }
        }
    }

    write_trait_dispatchers(&mut generated, &models, &mut ops);
    write_default_dispatcher(&mut generated, program, &models, &mut ops);

    let script = program
        .scripts
        .iter()
        .find(|script| script.source == program.source.entry())
        .expect("resolved single-source programs retain their root script");
    let mut parser = Parser::new_with_offset(
        engine,
        &script.text,
        models.for_source(script.source),
        ops.len(),
        patterns.len(),
        #[cfg(feature = "rust-import")]
        imported
            .as_mut()
            .map(|context| context.for_source(script.source)),
    );
    if let Some(scope) = scope {
        parser.bind_scope(scope);
    }
    let lowered = parser
        .parse()
        .map_err(|error| LoweringFailure::original(script.source, error.into()))?;
    let body = generated.body(
        script.source,
        source_full_span(program.source.text_at(script.source)),
        &lowered.source,
        &lowered.locations,
    );
    // The script's statements: a runtime error in them is reported at
    // the script's own text.
    retain_locations(&mut locations, body, None, script.source, lowered.locations);
    ops.extend(lowered.ops);
    patterns.extend(lowered.patterns);

    let mut ast = match engine.compile_legacy_script_with_scope_raw(
        scope,
        generated.as_str(),
        optimization_level,
    ) {
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
        .flat_map(ExecutableOp::dependencies)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let runtime_requirements = ops
        .iter()
        .filter_map(ExecutableOp::runtime_requirement)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let projection_requirements = ops
        .iter()
        .filter_map(ExecutableOp::projection_requirement)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let nominal_patterns: BTreeMap<ImmutableString, NominalPattern> = patterns
        .into_iter()
        .enumerate()
        .map(|(index, pattern)| (pattern_name(index).into(), pattern))
        .collect();

    for (index, op) in ops.into_iter().enumerate() {
        let sentinel: ImmutableString = format!("{SENTINEL}{index}").into();
        let private: ImmutableString = op.private_name(support_id).into();
        rewrites.insert(sentinel, private.clone());
        register(&mut module, private, op, &models);
    }
    // A debugger reads any value's `Debug` text through it (#488). A
    // program that formats with `Debug` registered it above; one that
    // doesn't carries it only while a debugger is registered (#526).
    debug_assert_eq!(
        format!("\0rawr.{}", PreludeOp::GenericDebug.name()),
        DEBUG_GENERIC
    );
    debug_assert_eq!(trait_dispatcher_name("Debug", "fmt"), DEBUG_DISPATCHER);
    #[cfg(feature = "debugging")]
    if engine.is_debugger_registered() {
        register(
            &mut module,
            DEBUG_GENERIC.into(),
            ExecutableOp::Prelude(PreludeOp::GenericDebug),
            &models,
        );
    }

    rewrite_statements(ast.statements_mut(), &rewrites, &nominal_patterns);
    #[cfg(feature = "debugging")]
    let named = engine.is_debugger_registered();
    #[cfg(not(feature = "debugging"))]
    let named = false;
    ast.for_each_fn_def_mut(|function| {
        rewrite_statements(function.body.statements_mut(), &rewrites, &nominal_patterns);
        if named {
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
            unsupported = crate::eval::machine::lower(function).err();
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

pub(super) struct Lowered {
    pub(super) source: String,
    pub(super) locations: Vec<location::Output>,
    pub(super) ops: Vec<ExecutableOp>,
    pub(super) patterns: Vec<NominalPattern>,
    pub(super) closures: Vec<closure::Lifted>,
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
