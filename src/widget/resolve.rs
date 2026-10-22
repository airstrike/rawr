//! Promotion of marked inherent implementations into lifecycle proofs.

use crate::resolve::{self, item, type_ref, LocatedError};
use crate::{application, host, method, receiver, type_def, widget, ImmutableString, Span};
use std::collections::BTreeMap;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

const CHILDREN: &str = "rawr_iced::widget::children::List";
const SIZE: &str = "rawr_iced::widget::layout::Size";
const LAYOUT_INPUT: &str = "rawr_iced::widget::layout::Input";
const LAYOUT_NODE: &str = "rawr_iced::widget::layout::Node";
const EVENT_INPUT: &str = "rawr_iced::widget::event::Input";
const EVENT_OUTCOME: &str = "rawr_iced::widget::event::Outcome";
const DRAW_INPUT: &str = "rawr_iced::widget::draw::Input";
const RENDERER: &str = "rawr_iced::Renderer";
const MOUSE_INPUT: &str = "rawr_iced::widget::mouse::Input";
const MOUSE_INTERACTION: &str = "iced::mouse::Interaction";
const OPERATION_INPUT: &str = "rawr_iced::widget::operation::Input";
const OPERATION_OUTCOME: &str = "rawr_iced::widget::operation::Outcome";
const OVERLAY_INPUT: &str = "rawr_iced::widget::overlay::Input";
const OVERLAY_VALUE: &str = "rawr_iced::widget::overlay::Value";

pub(crate) fn resolve(
    program: &resolve::Program,
    host: &host::Registry,
) -> Result<Box<[widget::Definition]>, LocatedError> {
    let definitions = program
        .types()
        .iter()
        .map(|definition| (definition.id(), definition))
        .collect::<BTreeMap<_, _>>();
    let methods = program
        .methods()
        .iter()
        .map(|definition| {
            (
                (
                    definition.source(),
                    definition.owner(),
                    definition.name().to_string(),
                ),
                definition,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let runtime = application::RuntimeGraph::new(program);
    let mut marked = BTreeMap::<type_def::Id, (crate::program::source::Id, Span)>::new();
    let mut protocols = Vec::new();

    for shell in program.items() {
        let item::Kind::Impl(item) = &shell.kind else {
            if let Some(attribute) = item_header(&shell.kind).and_then(|header| {
                header
                    .attributes
                    .iter()
                    .find(|attribute| attribute.is_widget())
            }) {
                return Err(primary(
                    shell.source,
                    invalid(
                        "widget",
                        "`#[widget]` may mark only an inherent impl",
                        attribute.span,
                        None,
                    ),
                ));
            }
            continue;
        };

        for function in &item.functions {
            if let Some(attribute) = function
                .header
                .attributes
                .iter()
                .find(|attribute| attribute.is_widget())
            {
                return Err(primary(
                    shell.source,
                    invalid(
                        &function.name.text,
                        "mark the containing impl, not an individual hook",
                        attribute.span,
                        Some(item.header.span),
                    ),
                ));
            }
        }

        let attributes = item
            .header
            .attributes
            .iter()
            .filter(|attribute| attribute.is_widget())
            .collect::<Vec<_>>();
        let Some(attribute) = attributes.first().copied() else {
            continue;
        };
        if attributes.len() != 1 {
            return Err(primary(
                shell.source,
                invalid(
                    "widget",
                    "a lifecycle impl has exactly one `#[widget]` marker",
                    attributes[1].span,
                    Some(attribute.span),
                ),
            ));
        }
        if !attribute.arguments.is_empty() {
            return Err(primary(
                shell.source,
                invalid(
                    "widget",
                    "the `#[widget]` marker takes no arguments",
                    attribute.span,
                    None,
                ),
            ));
        }

        let owner_id = script_owner(&item.owner).map_err(|error| primary(shell.source, error))?;
        let owner = definitions[&owner_id];
        let dependency = host.facade_dependency("rawr_iced").ok_or_else(|| {
            primary(
                shell.source,
                invalid(
                    owner.name(),
                    "a marked widget requires one exact installed `rawr_iced` facade dependency",
                    attribute.span,
                    None,
                ),
            )
        })?;
        let iced = host.facade_dependency("iced").ok_or_else(|| {
            primary(
                shell.source,
                invalid(
                    owner.name(),
                    "a marked widget requires one exact installed `iced` facade dependency",
                    attribute.span,
                    None,
                ),
            )
        })?;
        if let Some((previous_source, previous)) =
            marked.insert(owner_id, (shell.source, attribute.span))
        {
            return Err(LocatedError::related(
                shell.source,
                previous_source,
                invalid(
                    owner.name(),
                    "a nominal widget has exactly one marked lifecycle impl",
                    attribute.span,
                    Some(previous),
                ),
            ));
        }

        let mut declared = BTreeMap::new();
        for function in &item.functions {
            let Some(kind) = widget::hook::Kind::parse(&function.name.text) else {
                return Err(primary(
                    shell.source,
                    invalid(
                        owner.name(),
                        format!(
                            "unsupported lifecycle hook `{}`; ordinary methods belong in an unmarked impl",
                            function.name.text
                        ),
                        function.name.span,
                        Some(attribute.span),
                    ),
                ));
            };
            let method = methods
                .get(&(shell.source, owner_id, function.name.text.to_string()))
                .copied()
                .expect("every resolved impl function retains one method definition");
            declared.insert(kind, method);
        }

        let state = match declared.get(&widget::hook::Kind::State).copied() {
            Some(method) => validate_state(method, &definitions, &runtime)
                .map_err(|error| primary(shell.source, error))?,
            None => widget::state::Type::Unit,
        };
        let mut message = Constraint::default();
        let mut hooks = Vec::with_capacity(declared.len());
        for (kind, method) in declared {
            validate_hook(
                kind,
                method,
                &state,
                &dependency,
                &iced,
                &definitions,
                &runtime,
                &mut message,
            )
            .map_err(|error| primary(shell.source, error))?;
            let output = application::Type::from_resolved(method.output(), &definitions, &runtime)
                .exact_runtime_type()
                .ok_or_else(|| {
                    primary(
                        shell.source,
                        invalid(
                            owner.name(),
                            format!(
                                "lifecycle hook `{}` output has no exact runtime representation",
                                method.name()
                            ),
                            method.name_span(),
                            Some(attribute.span),
                        ),
                    )
                })?;
            hooks.push(widget::hook::Definition::new(
                kind,
                symbolic(method, owner),
                location(program, method.source(), method.name_span()),
                output,
            ));
        }

        protocols.push(widget::Definition::new(
            type_def::Identity::Script(owner.key().clone()),
            owner.schema().clone(),
            iced,
            dependency,
            state,
            message.finish(),
            hooks.into_boxed_slice(),
            location(program, shell.source, attribute.span),
        ));
    }

    protocols.sort_by(|left, right| left.owner().cmp(right.owner()));
    Ok(protocols.into_boxed_slice())
}

#[derive(Default)]
struct Constraint {
    exact: Option<(application::Type, ImmutableString, Span)>,
}

impl Constraint {
    fn constrain(
        &mut self,
        owner: &str,
        hook: &method::Definition,
        ty: application::Type,
    ) -> Result<(), resolve::Error> {
        if let Some((current, previous_hook, previous)) = &self.exact {
            if current != &ty {
                return Err(invalid(
                    owner,
                    format!(
                        "hook `{}` constrains Message to `{}`, but hook `{previous_hook}` established `{}`",
                        hook.name(),
                        ty.display_name(),
                        current.display_name()
                    ),
                    hook.name_span(),
                    Some(*previous),
                ));
            }
        } else {
            self.exact = Some((ty, hook.name().into(), hook.name_span()));
        }
        Ok(())
    }

    fn finish(self) -> widget::message::Type {
        match self.exact {
            Some((ty, ..)) => widget::message::Type::Exact(ty),
            None => widget::message::Type::Generic,
        }
    }
}

fn validate_state(
    method: &method::Definition,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &application::RuntimeGraph,
) -> Result<widget::state::Type, resolve::Error> {
    expect_receiver(method, receiver::Mode::Shared)?;
    expect_parameter_count(method, 0)?;
    let type_ref::Type::Path(path) = method.output() else {
        return Err(signature_error(
            method,
            "`state` must return one nominal script type",
            type_span(method.output()),
        ));
    };
    if !matches!(&path.target, type_ref::Target::Script { .. }) {
        return Err(signature_error(
            method,
            "`state` must return one nominal script type",
            path.span,
        ));
    }
    Ok(widget::state::Type::nominal(
        application::Type::from_resolved(method.output(), definitions, runtime),
    ))
}

fn validate_hook(
    kind: widget::hook::Kind,
    method: &method::Definition,
    state: &widget::state::Type,
    dependency: &host::facade::Dependency,
    iced: &host::facade::Dependency,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &application::RuntimeGraph,
    message: &mut Constraint,
) -> Result<(), resolve::Error> {
    let Some(signature) = hook_signature(kind) else {
        return Ok(());
    };
    expect_receiver(method, signature.receiver)?;

    let mut parameter = 0;
    if let (Some(mode), widget::state::Type::Nominal(state)) = (signature.state, state) {
        let mutable = mode == receiver::Mode::Mutable;
        let Some(actual) = method.parameters().get(parameter) else {
            return Err(signature_error(
                method,
                format!(
                    "stateful `{}` requires `{}` as its first parameter",
                    kind.name(),
                    reference_name(state, mutable)
                ),
                method.name_span(),
            ));
        };
        expect_state_reference(method, &actual.ty, state, mutable, definitions, runtime)?;
        parameter += 1;
    }

    if let Some(expected) = signature.input {
        let Some(actual) = method.parameters().get(parameter) else {
            return Err(signature_error(
                method,
                format!("`{}` requires one `{expected}` input", kind.name()),
                method.name_span(),
            ));
        };
        expect_host(method, &actual.ty, expected, 0, dependency)?;
        parameter += 1;
    }
    if signature.renderer {
        let Some(actual) = method.parameters().get(parameter) else {
            return Err(signature_error(
                method,
                "`draw` requires `&mut rawr_iced::Renderer` before `draw::Input`",
                method.name_span(),
            ));
        };
        expect_renderer_reference(method, &actual.ty, dependency)?;
        parameter += 1;

        let Some(actual) = method.parameters().get(parameter) else {
            return Err(signature_error(
                method,
                "`draw` requires one `rawr_iced::widget::draw::Input` after the renderer",
                method.name_span(),
            ));
        };
        expect_host(method, &actual.ty, DRAW_INPUT, 0, dependency)?;
        parameter += 1;
    }
    expect_parameter_count(method, parameter)?;

    match signature.output {
        Output::Message(expected) => {
            let ty = expect_host(method, method.output(), expected, 1, dependency)?[0];
            message.constrain(
                method.owner_name(),
                method,
                application::Type::from_resolved(ty, definitions, runtime),
            )?;
        }
        Output::Host(expected) => {
            let owner = if expected == MOUSE_INTERACTION {
                iced
            } else {
                dependency
            };
            expect_host(method, method.output(), expected, 0, owner)?;
        }
        Output::Unit => expect_unit(method)?,
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct HookSignature {
    receiver: receiver::Mode,
    state: Option<receiver::Mode>,
    input: Option<&'static str>,
    renderer: bool,
    output: Output,
}

#[derive(Clone, Copy)]
enum Output {
    Unit,
    Host(&'static str),
    Message(&'static str),
}

fn hook_signature(kind: widget::hook::Kind) -> Option<HookSignature> {
    use widget::hook::Kind;

    Some(match kind {
        Kind::State => return None,
        Kind::Children => HookSignature {
            receiver: receiver::Mode::Shared,
            state: None,
            input: None,
            renderer: false,
            output: Output::Message(CHILDREN),
        },
        Kind::Diff => HookSignature {
            receiver: receiver::Mode::Mutable,
            state: Some(receiver::Mode::Mutable),
            input: None,
            renderer: false,
            output: Output::Unit,
        },
        Kind::Size => HookSignature {
            receiver: receiver::Mode::Shared,
            state: None,
            input: None,
            renderer: false,
            output: Output::Host(SIZE),
        },
        Kind::Layout => HookSignature {
            receiver: receiver::Mode::Mutable,
            state: Some(receiver::Mode::Mutable),
            input: Some(LAYOUT_INPUT),
            renderer: false,
            output: Output::Host(LAYOUT_NODE),
        },
        Kind::Update => HookSignature {
            receiver: receiver::Mode::Mutable,
            state: Some(receiver::Mode::Mutable),
            input: Some(EVENT_INPUT),
            renderer: false,
            output: Output::Message(EVENT_OUTCOME),
        },
        Kind::Draw => HookSignature {
            receiver: receiver::Mode::Shared,
            state: Some(receiver::Mode::Shared),
            input: None,
            renderer: true,
            output: Output::Unit,
        },
        Kind::MouseInteraction => HookSignature {
            receiver: receiver::Mode::Shared,
            state: Some(receiver::Mode::Shared),
            input: Some(MOUSE_INPUT),
            renderer: false,
            output: Output::Host(MOUSE_INTERACTION),
        },
        Kind::Operate => HookSignature {
            receiver: receiver::Mode::Mutable,
            state: Some(receiver::Mode::Mutable),
            input: Some(OPERATION_INPUT),
            renderer: false,
            output: Output::Host(OPERATION_OUTCOME),
        },
        Kind::Overlay => HookSignature {
            receiver: receiver::Mode::Mutable,
            state: Some(receiver::Mode::Mutable),
            input: Some(OVERLAY_INPUT),
            renderer: false,
            output: Output::Message(OVERLAY_VALUE),
        },
    })
}

fn expect_renderer_reference(
    method: &method::Definition,
    actual: &type_ref::Type,
    dependency: &host::facade::Dependency,
) -> Result<(), resolve::Error> {
    let type_ref::Type::Reference {
        mutable: true,
        target,
        span,
        ..
    } = actual
    else {
        return Err(signature_error(
            method,
            "expected exact mutable `&mut rawr_iced::Renderer` scoped authority",
            type_span(actual),
        ));
    };
    let type_ref::Type::Path(path) = target.as_ref() else {
        return Err(signature_error(
            method,
            "expected exact mutable `&mut rawr_iced::Renderer` scoped authority",
            *span,
        ));
    };
    let type_ref::Target::Host { definition, owner } = &path.target else {
        return Err(signature_error(
            method,
            "expected exact mutable `&mut rawr_iced::Renderer` scoped authority",
            path.span,
        ));
    };
    if definition.path() != RENDERER
        || definition.storage() != host::type_def::Storage::Invocation
        || owner.as_ref() != Some(dependency)
        || !path.arguments.is_empty()
    {
        return Err(signature_error(
            method,
            "expected facade-authenticated invocation-scoped `&mut rawr_iced::Renderer`",
            path.span,
        ));
    }
    Ok(())
}

fn expect_receiver(
    method: &method::Definition,
    expected: receiver::Mode,
) -> Result<(), resolve::Error> {
    if method.receiver() == Some(expected) {
        return Ok(());
    }
    Err(signature_error(
        method,
        format!(
            "`{}` requires `{}`",
            method.name(),
            match expected {
                receiver::Mode::Owned => "self",
                receiver::Mode::Shared => "&self",
                receiver::Mode::Mutable => "&mut self",
            }
        ),
        method.receiver_span().unwrap_or(method.name_span()),
    ))
}

fn expect_parameter_count(
    method: &method::Definition,
    expected: usize,
) -> Result<(), resolve::Error> {
    if method.parameters().len() == expected {
        return Ok(());
    }
    Err(signature_error(
        method,
        format!(
            "expected {expected} lifecycle parameter(s), found {}",
            method.parameters().len()
        ),
        method.name_span(),
    ))
}

fn expect_state_reference(
    method: &method::Definition,
    actual: &type_ref::Type,
    state: &application::Type,
    mutable: bool,
    definitions: &BTreeMap<type_def::Id, &type_def::Definition>,
    runtime: &application::RuntimeGraph,
) -> Result<(), resolve::Error> {
    let type_ref::Type::Reference {
        mutable: actual_mutable,
        target,
        span,
        ..
    } = actual
    else {
        return Err(signature_error(
            method,
            format!("expected `{}`", reference_name(state, mutable)),
            type_span(actual),
        ));
    };
    if *actual_mutable != mutable
        || application::Type::from_resolved(target, definitions, runtime) != *state
    {
        return Err(signature_error(
            method,
            format!("expected `{}`", reference_name(state, mutable)),
            *span,
        ));
    }
    Ok(())
}

fn expect_host<'a>(
    method: &method::Definition,
    actual: &'a type_ref::Type,
    expected: &str,
    arguments: usize,
    dependency: &host::facade::Dependency,
) -> Result<Vec<&'a type_ref::Type>, resolve::Error> {
    let type_ref::Type::Path(path) = actual else {
        return Err(signature_error(
            method,
            format!("expected `{expected}`"),
            type_span(actual),
        ));
    };
    let type_ref::Target::Host { definition, owner } = &path.target else {
        return Err(signature_error(
            method,
            format!("expected `{expected}`"),
            path.span,
        ));
    };
    let actual_arguments = path
        .arguments
        .iter()
        .filter_map(|argument| match argument {
            type_ref::Argument::Type(ty) => Some(ty),
            type_ref::Argument::Lifetime(_) => None,
        })
        .collect::<Vec<_>>();
    if definition.path() != expected || actual_arguments.len() != arguments {
        return Err(signature_error(
            method,
            format!(
                "expected `{expected}{}`",
                if arguments == 0 { "" } else { "<Message>" }
            ),
            path.span,
        ));
    }
    let Some(owner) = owner else {
        return Err(signature_error(
            method,
            format!(
                "expected `{expected}` from the exact installed `{}` facade; legacy or unowned lookalikes are not lifecycle proofs",
                dependency.root()
            ),
            path.span,
        ));
    };
    if owner != dependency {
        return Err(signature_error(
            method,
            format!(
                "expected `{expected}` from the same exact `{}` facade dependency as this widget protocol",
                dependency.root()
            ),
            path.span,
        ));
    }
    Ok(actual_arguments)
}

fn expect_unit(method: &method::Definition) -> Result<(), resolve::Error> {
    if matches!(method.output(), type_ref::Type::Unit(_)) {
        Ok(())
    } else {
        Err(signature_error(
            method,
            "expected unit output `()`",
            type_span(method.output()),
        ))
    }
}

fn symbolic(method: &method::Definition, owner: &type_def::Definition) -> crate::item::Value {
    crate::item::Value::new(
        method.key().clone(),
        crate::item::Kind::Method,
        Some(method.owner_identity().clone()),
        method.receiver(),
        method.fingerprint().clone(),
        owner.capabilities().fingerprint(),
    )
}

fn script_owner(ty: &type_ref::Type) -> Result<type_def::Id, resolve::Error> {
    let type_ref::Type::Path(path) = ty else {
        return Err(resolve::Error::InvalidImplOwner {
            owner: type_name(ty),
            reference: type_span(ty),
        });
    };
    let type_ref::Target::Script { definition, .. } = &path.target else {
        return Err(resolve::Error::InvalidImplOwner {
            owner: path.name.clone(),
            reference: path.span,
        });
    };
    Ok(*definition)
}

fn item_header(kind: &item::Kind) -> Option<&crate::item::Header> {
    Some(match kind {
        item::Kind::Use(item) => &item.header,
        item::Kind::Struct(item) => &item.header,
        item::Kind::Enum(item) => &item.header,
        item::Kind::Impl(item) => &item.header,
        item::Kind::Trait(item) => &item.header,
        item::Kind::Function(item) => &item.header,
        item::Kind::LegacyFunction(_) => return None,
    })
}

fn location(
    program: &resolve::Program,
    source: crate::program::source::Id,
    span: Span,
) -> widget::Location {
    widget::Location::new(program.source().table().key(source.index()), span)
}

fn reference_name(state: &application::Type, mutable: bool) -> String {
    format!(
        "&{}{}",
        if mutable { "mut " } else { "" },
        state.display_name()
    )
}

fn signature_error(
    method: &method::Definition,
    reason: impl Into<ImmutableString>,
    reference: Span,
) -> resolve::Error {
    invalid(
        method.owner_name(),
        format!("invalid `{}` signature: {}", method.name(), reason.into()),
        reference,
        Some(method.name_span()),
    )
}

fn invalid(
    owner: impl Into<ImmutableString>,
    reason: impl Into<ImmutableString>,
    reference: Span,
    declaration: Option<Span>,
) -> resolve::Error {
    resolve::Error::InvalidWidget {
        widget: owner.into(),
        reason: reason.into(),
        reference,
        declaration,
    }
}

fn primary(source: crate::program::source::Id, error: resolve::Error) -> LocatedError {
    LocatedError::primary(source, error)
}

fn type_span(ty: &type_ref::Type) -> Span {
    match ty {
        type_ref::Type::Unit(span)
        | type_ref::Type::Tuple { span, .. }
        | type_ref::Type::Reference { span, .. } => *span,
        type_ref::Type::Path(path) => path.span,
    }
}

fn type_name(ty: &type_ref::Type) -> ImmutableString {
    match ty {
        type_ref::Type::Unit(_) => "()".into(),
        type_ref::Type::Tuple { .. } => "tuple".into(),
        type_ref::Type::Path(path) => path.name.clone(),
        type_ref::Type::Reference {
            mutable, target, ..
        } => format!(
            "&{}{}",
            if *mutable { "mut " } else { "" },
            type_name(target)
        )
        .into(),
    }
}
