//! Exact compiler evidence selected by the iced profile.

use rawr_import::api::{identity, ledger, model};
use rawr_import::profile;
use sha2::{Digest as _, Sha256};

#[cfg(feature = "profile-selection-diagnostic")]
use std::cell::Cell;

const ICED_SOURCE: &str = "git+https://github.com/iced-rs/iced?rev=be88873d38544455e26c7a104eb757da2cbbac6b#be88873d38544455e26c7a104eb757da2cbbac6b";
const ICED_VERSION: &str = "0.15.0-dev";

/// Export paths below the `iced` alias that selection looks up.
const ELEMENT: [&str; 1] = ["Element"];
const SOURCE: [&str; 2] = ["widget", "Text"];
const OPERATION: [&str; 2] = ["widget", "text"];

/// Every iced path whose item selection below reads, so an
/// extraction seeded from demand always holds them. Export lookups are
/// demanded by their own paths (`ELEMENT`, `SOURCE`, `OPERATION`, the ABI
/// roles); definitions found by display path are demanded through an iced
/// spelling: `Widget` (iced_core's trait) and `Theme` (iced_core's enum).
/// Kept beside the lookups; `demand_covers_every_lookup` checks it.
pub(super) const DEMAND: &[&str] = &[
    "iced::Color",
    "iced::Element",
    "iced::Length",
    "iced::Rectangle",
    "iced::Theme",
    "iced::Vector",
    "iced::advanced::Widget",
    "iced::widget::Text",
    "iced::widget::text",
];

/// Closed iced extension signature role selected only by this profile.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum AbiRole {
    Color,
    Length,
    Rectangle,
    Vector,
}

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum Diagnostic {
    ElementExport = 1,
    ElementDefinition = 2,
    WidgetDefinition = 3,
    WidgetImplementations = 6,
    PairBinding = 8,
    TargetArguments = 9,
    ContractEvidence = 10,
    SourceExport = 11,
    SourceAlias = 12,
    OperationEndpoint = 14,
    ColorAbi = 15,
    LengthAbi = 16,
    RectangleAbi = 17,
    VectorAbi = 18,
}

#[cfg(feature = "profile-selection-diagnostic")]
std::thread_local! {
    static SELECTION_DIAGNOSTIC: Cell<u8> = const { Cell::new(0) };
}

pub(super) struct Set<'graph> {
    widget: &'graph model::Definition,
    element: Element<'graph>,
    widgets: Vec<&'graph model::Implementation>,
    projections: Vec<Pair<'graph>>,
    source: Source<'graph>,
    abis: [Abi<'graph>; 4],
    operation: Operation<'graph>,
}

struct Element<'graph> {
    export: &'graph ledger::Path,
    alias: &'graph model::Definition,
    target: &'graph model::Definition,
}

struct Source<'graph> {
    export: &'graph ledger::Path,
    alias: &'graph model::Definition,
    target: &'graph model::Definition,
}

struct Operation<'graph> {
    export: &'graph ledger::Path,
    definition: &'graph model::Definition,
    theme: &'graph model::Definition,
}

pub(super) struct Abi<'graph> {
    role: AbiRole,
    export: &'graph ledger::Path,
    definition: &'graph model::Definition,
    arguments: Box<[profile::contract::AbiArgument]>,
}

pub(super) struct Pair<'graph> {
    source: &'graph model::Definition,
    target: &'graph model::Definition,
    widget: &'graph model::Implementation,
    arguments: Box<[profile::contract::Argument]>,
}

impl<'graph> Set<'graph> {
    pub(super) fn select(
        graph: &'graph rawr_import::api::Graph<impl rawr_import::api::Phase>,
    ) -> Result<Self, profile::Error> {
        reset_diagnostic();
        let widget = diagnosed(
            unique_definition(graph, "iced_core", &["Widget"], trait_definition),
            Diagnostic::WidgetDefinition,
        )?;
        let element = diagnosed(select_element(graph), Diagnostic::ElementDefinition)?;
        let source = diagnosed(select_source(graph), Diagnostic::SourceAlias)?;

        // On iced master a widget becomes an Element through the Widget
        // trait's own provided `_boxed`, so the Widget implementation alone is
        // the projection's evidence; there is no `From<W> for Element`.
        let mut widgets = graph
            .implementations()
            .filter(|implementation| {
                implements(implementation, widget.id())
                    && type_definition(implementation.self_type()) == Some(source.target.id())
            })
            .collect::<Vec<_>>();
        if widgets.is_empty() {
            return Err(foreign(Diagnostic::WidgetImplementations));
        }
        widgets.sort_unstable_by_key(|implementation| implementation.key());

        let mut projections = Vec::new();
        for widget in &widgets {
            projections
                .try_reserve_exact(1)
                .map_err(|_| profile::Error::Allocation)?;
            let arguments = diagnosed(
                target_arguments(source.target, widget, element.target),
                Diagnostic::TargetArguments,
            )?;
            projections.push(Pair {
                source: source.target,
                target: element.target,
                widget,
                arguments,
            });
        }
        if projections.len() != 1 {
            return Err(canonical(Diagnostic::PairBinding));
        }
        let abis = [
            select_abi(graph, AbiRole::Color)?,
            select_abi(graph, AbiRole::Length)?,
            select_abi(graph, AbiRole::Rectangle)?,
            select_abi(graph, AbiRole::Vector)?,
        ];
        let operation = diagnosed(
            select_text_operation(graph, source.target),
            Diagnostic::OperationEndpoint,
        )?;

        Ok(Self {
            widget,
            element,
            widgets,
            projections,
            source,
            abis,
            operation,
        })
    }

    pub(super) const fn widget(&self) -> &'graph model::Definition {
        self.widget
    }

    pub(super) const fn element(&self) -> &'graph model::Definition {
        self.element.target
    }

    pub(super) const fn element_alias(&self) -> &'graph model::Definition {
        self.element.alias
    }

    pub(super) const fn element_export(&self) -> &'graph ledger::Path {
        self.element.export
    }

    pub(super) fn widgets(
        &self,
    ) -> impl ExactSizeIterator<Item = &'graph model::Implementation> + '_ {
        self.widgets.iter().copied()
    }

    pub(super) fn projections(&self) -> impl ExactSizeIterator<Item = &Pair<'graph>> {
        self.projections.iter()
    }

    pub(super) const fn source_export(&self) -> &'graph ledger::Path {
        self.source.export
    }
    pub(super) const fn source_alias(&self) -> &'graph model::Definition {
        self.source.alias
    }
    pub(super) const fn source(&self) -> &'graph model::Definition {
        self.source.target
    }

    pub(super) fn abis(&self) -> impl ExactSizeIterator<Item = &Abi<'graph>> {
        self.abis.iter()
    }

    pub(super) const fn operation_export(&self) -> &'graph ledger::Path {
        self.operation.export
    }

    pub(super) const fn operation_definition(&self) -> &'graph model::Definition {
        self.operation.definition
    }

    pub(super) const fn operation_theme(&self) -> &'graph model::Definition {
        self.operation.theme
    }
}

impl AbiRole {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Color => "Color",
            Self::Length => "Length",
            Self::Rectangle => "Rectangle",
            Self::Vector => "Vector",
        }
    }

    const fn diagnostic(self) -> Diagnostic {
        match self {
            Self::Color => Diagnostic::ColorAbi,
            Self::Length => Diagnostic::LengthAbi,
            Self::Rectangle => Diagnostic::RectangleAbi,
            Self::Vector => Diagnostic::VectorAbi,
        }
    }

    const fn module(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Length => "length",
            Self::Rectangle => "rectangle",
            Self::Vector => "vector",
        }
    }

    pub(super) fn linked_fingerprint(self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"rawr.iced.extension-abi.linked-host/v4");
        digest.update(16_u32.to_le_bytes());
        digest.update(self.name().as_bytes());
        digest.update(match self {
            Self::Color => b"iced::Color".as_slice(),
            Self::Length => b"iced::Length".as_slice(),
            Self::Rectangle => b"iced::Rectangle<f32>".as_slice(),
            Self::Vector => b"iced::Vector<f32>".as_slice(),
        });
        if self == Self::Length {
            digest.update(b"profile-semantic/iced::Length/Fill/Fixed(i64)/v1");
        } else {
            digest.update(b"profile-semantic-support/v1");
        }
        digest.finalize().into()
    }
}

pub(super) fn operation_linked_fingerprint() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.iced.operation-endpoint.linked-host/v3");
    digest.update(8_u32.to_le_bytes());
    digest.update(b"iced::widget::text");
    digest.update(b"owned String -> &'view str -> iced::widget::Text<'view, Theme>");
    digest.update(b"TextProxy/owned-immutable-string/v1");
    for hook in [
        b"tag".as_slice(),
        b"state",
        b"size",
        b"layout",
        b"update",
        b"mouse_interaction",
        b"draw",
        b"operate",
    ] {
        digest.update(hook.len().to_le_bytes());
        digest.update(hook);
    }
    for inherited in [b"diff".as_slice(), b"overlay"] {
        digest.update(inherited.len().to_le_bytes());
        digest.update(inherited);
    }
    digest.update(b"caller-owned Tree retains text paragraph state");
    digest.finalize().into()
}

impl<'graph> Abi<'graph> {
    pub(super) const fn role(&self) -> AbiRole {
        self.role
    }

    pub(super) const fn export(&self) -> &'graph ledger::Path {
        self.export
    }

    pub(super) const fn definition(&self) -> &'graph model::Definition {
        self.definition
    }

    pub(super) const fn arguments(&self) -> &[profile::contract::AbiArgument] {
        &self.arguments
    }
}

impl<'graph> Pair<'graph> {
    pub(super) const fn source(&self) -> &'graph model::Definition {
        self.source
    }

    pub(super) const fn target(&self) -> &'graph model::Definition {
        self.target
    }

    pub(super) const fn widget(&self) -> &'graph model::Implementation {
        self.widget
    }

    pub(super) const fn arguments(&self) -> &[profile::contract::Argument] {
        &self.arguments
    }
}

/// The Element's `Message` comes from the source's own parameter when the
/// Widget implementation's `Message` is one of them, and is fresh otherwise
/// (iced's `Text` has no `Message`).
fn target_arguments(
    source: &model::Definition,
    widget: &model::Implementation,
    target: &model::Definition,
) -> Result<Box<[profile::contract::Argument]>, profile::Error> {
    if type_parameters(target).next().is_none() {
        return Ok(Box::default());
    }
    let model::Trait::Trait { arguments, .. } = widget.trait_ref() else {
        return Err(profile::Error::ForeignEvidence);
    };
    // rustc normalizes the trait arguments with a leading `Self`.
    let message = match arguments.as_ref() {
        [model::Argument::Type(self_argument), model::Argument::Type(message), ..]
            if self_argument == widget.self_type() =>
        {
            Some(message)
        }
        _ => None,
    };
    let source_argument = message.and_then(|message| {
        type_parameters(source).position(|parameter| {
            definition_argument(widget.self_type(), source, parameter)
                .is_some_and(|argument| argument == message)
        })
    });
    let argument = match source_argument {
        Some(index) => profile::contract::Argument::source(
            u32::try_from(index).map_err(|_| profile::Error::ForeignEvidence)?,
        ),
        None => profile::contract::Argument::fresh(),
    };
    Ok(Box::from([argument]))
}

fn type_parameters(definition: &model::Definition) -> impl Iterator<Item = &model::Parameter> {
    generics(definition)
        .into_iter()
        .flat_map(model::Generics::parameters)
        .filter(|parameter| matches!(parameter.kind(), model::ParameterKind::Type { .. }))
}

fn generics(definition: &model::Definition) -> Option<&model::Generics> {
    match definition.kind() {
        model::Kind::Struct { generics, .. }
        | model::Kind::Union { generics, .. }
        | model::Kind::Enum { generics, .. }
        | model::Kind::TypeAlias { generics, .. }
        | model::Kind::Function { generics, .. }
        | model::Kind::Trait { generics, .. }
        | model::Kind::AssociatedType { generics, .. } => Some(generics),
        model::Kind::Rejected { .. }
        | model::Kind::Module
        | model::Kind::Const { .. }
        | model::Kind::TypeConst { .. }
        | model::Kind::Static { .. }
        | model::Kind::Sysroot { .. } => None,
    }
}

fn definition_argument<'a>(
    ty: &'a model::Type,
    definition: &model::Definition,
    parameter: &model::Parameter,
) -> Option<&'a model::Type> {
    let model::Type::Definition { id, arguments } = ty else {
        return None;
    };
    if id != definition.id() {
        return None;
    }
    generics(definition)?
        .parameters()
        .zip(arguments)
        .find_map(|(candidate, argument)| {
            (candidate.owner() == parameter.owner() && candidate.index() == parameter.index())
                .then_some(argument)
        })
        .and_then(|argument| match argument {
            model::Argument::Type(ty) => Some(ty),
            model::Argument::Lifetime(_) | model::Argument::Const(_) => None,
        })
}

fn select_element(
    graph: &rawr_import::api::Graph<impl rawr_import::api::Phase>,
) -> Result<Element<'_>, profile::Error> {
    let mut selected = graph.exports().filter(|outcome| {
        let key = outcome.key();
        key.root_alias() == "iced"
            && key.namespace() == identity::Namespace::Type
            && key.path().eq(ELEMENT)
    });
    let outcome = selected
        .next()
        .ok_or_else(|| foreign(Diagnostic::ElementExport))?;
    if selected.next().is_some() {
        return Err(canonical(Diagnostic::ElementExport));
    }
    let ledger::Path::Imported(export) = outcome else {
        return Err(foreign(Diagnostic::ElementExport));
    };
    // iced master's `iced::Element` is iced_widget's alias, re-exported.
    let alias = exact_definition(graph, export.target())?;
    if !pinned_package(alias, "iced_widget")
        || !matches!(alias.owner(), model::Owner::Free)
        || !matches!(alias.visibility(), model::Visibility::Public)
    {
        return Err(profile::Error::ForeignEvidence);
    }
    let model::Kind::TypeAlias {
        ty,
        generics: alias_generics,
    } = alias.kind()
    else {
        return Err(profile::Error::ForeignEvidence);
    };
    let model::Type::Definition { id, arguments } = ty else {
        return Err(profile::Error::ForeignEvidence);
    };
    let target = exact_definition(graph, id)?;
    let model::Kind::Struct {
        generics: target_generics,
        ..
    } = target.kind()
    else {
        return Err(profile::Error::ForeignEvidence);
    };
    // Without a `From` impl to pin it, the target is named exactly, so the
    // overlay `Element` sibling (same package, same generics) is refused.
    if !pinned_package(target, "iced_core")
        || target
            .display_path()
            .ne(["iced_core", "widget", "element", "Element"])
        || !matches!(target.owner(), model::Owner::Free)
        || !matches!(target.visibility(), model::Visibility::Public)
        || !exact_alias_arguments(alias, alias_generics, target, target_generics, arguments)
    {
        return Err(profile::Error::ForeignEvidence);
    }
    Ok(Element {
        export: outcome,
        alias,
        target,
    })
}

fn select_source(
    graph: &rawr_import::api::Graph<impl rawr_import::api::Phase>,
) -> Result<Source<'_>, profile::Error> {
    let mut selected = graph.exports().filter(|outcome| {
        let key = outcome.key();
        key.root_alias() == "iced"
            && key.namespace() == identity::Namespace::Type
            && key.path().eq(SOURCE)
    });
    let outcome = selected
        .next()
        .ok_or_else(|| foreign(Diagnostic::SourceExport))?;
    if selected.next().is_some() {
        return Err(canonical(Diagnostic::SourceExport));
    }
    let ledger::Path::Imported(export) = outcome else {
        return Err(foreign(Diagnostic::SourceExport));
    };
    let alias = exact_definition(graph, export.target())?;
    if !pinned_package(alias, "iced_widget")
        || !matches!(alias.owner(), model::Owner::Free)
        || !matches!(alias.visibility(), model::Visibility::Public)
    {
        return Err(foreign(Diagnostic::SourceAlias));
    }
    let model::Kind::TypeAlias { ty, generics } = alias.kind() else {
        return Err(foreign(Diagnostic::SourceAlias));
    };
    let model::Type::Definition { id, arguments } = ty else {
        return Err(foreign(Diagnostic::SourceAlias));
    };
    let expected = exact_definition(graph, id)?;
    let model::Kind::Struct {
        generics: target_generics,
        ..
    } = expected.kind()
    else {
        return Err(foreign(Diagnostic::SourceAlias));
    };
    if !pinned_package(expected, "iced_core")
        || !matches!(expected.owner(), model::Owner::Free)
        || !matches!(expected.visibility(), model::Visibility::Public)
        || !exact_alias_arguments(alias, generics, expected, target_generics, arguments)
    {
        return Err(foreign(Diagnostic::SourceAlias));
    }
    Ok(Source {
        export: outcome,
        alias,
        target: expected,
    })
}

fn select_text_operation<'graph>(
    graph: &'graph rawr_import::api::Graph<impl rawr_import::api::Phase>,
    source: &model::Definition,
) -> Result<Operation<'graph>, profile::Error> {
    let mut exports = graph.exports().filter(|outcome| {
        let key = outcome.key();
        key.root_alias() == "iced"
            && key.namespace() == identity::Namespace::Value
            && key.path().eq(OPERATION)
    });
    let export = exports.next().ok_or(profile::Error::ForeignEvidence)?;
    if exports.next().is_some() {
        return Err(profile::Error::Canonical);
    }
    let ledger::Path::Imported(imported) = export else {
        return Err(profile::Error::ForeignEvidence);
    };
    let definition = exact_definition(graph, imported.target())?;
    if !pinned_package(definition, "iced_widget")
        || definition
            .display_path()
            .ne(["iced_widget", "helpers", "text"])
        || !matches!(definition.owner(), model::Owner::Free)
        || !matches!(definition.visibility(), model::Visibility::Public)
        || !exact_text_signature(definition, source)
    {
        return Err(profile::Error::ForeignEvidence);
    }
    let theme = unique_definition(graph, "iced_core", &["theme", "Theme"], |definition| {
        matches!(definition.kind(), model::Kind::Enum { .. })
    })?;
    Ok(Operation {
        export,
        definition,
        theme,
    })
}

/// iced master's `pub fn text<'a, Theme>(text: impl IntoFragment<'a>) ->
/// Text<'a, Theme> where Theme: Catalog + 'a`; the `impl` argument is the
/// third, synthetic, type parameter.
fn exact_text_signature(definition: &model::Definition, source: &model::Definition) -> bool {
    let model::Kind::Function {
        signature,
        generics,
    } = definition.kind()
    else {
        return false;
    };
    let owner = definition.id();
    let parameters = generics.parameters().collect::<Vec<_>>();
    let predicates = generics.predicates().collect::<Vec<_>>();
    let one = |matches: &dyn Fn(&model::Predicate) -> bool| {
        predicates
            .iter()
            .filter(|predicate| matches(predicate))
            .count()
            == 1
    };
    signature.safety() == model::Safety::Safe
        && !signature.const_function()
        && !signature.async_function()
        && signature.abi() == "\"Rust\""
        && !signature.variadic()
        && signature.binders().len() == 0
        && matches!(signature.receiver(), model::Receiver::None)
        && matches!(
            signature.inputs().collect::<Vec<_>>().as_slice(),
            [model::Type::Parameter { owner: input_owner, index: 2 }] if input_owner == owner
        )
        && exact_text_output(signature.output(), owner, source.id())
        && parameters.len() == 3
        && parameters.iter().enumerate().all(|(index, parameter)| {
            parameter.owner() == owner
                && parameter.index() == u32::try_from(index).unwrap_or(u32::MAX)
                && match (index, parameter.kind()) {
                    (0, model::ParameterKind::Lifetime { bounds }) => bounds.is_empty(),
                    (
                        1 | 2,
                        model::ParameterKind::Type {
                            bounds,
                            default: model::Default::Absent,
                        },
                    ) => bounds.is_empty(),
                    _ => false,
                }
        })
        && predicates.len() == 3
        && one(&|predicate| exact_parameter_trait(predicate, owner, 1, None))
        && one(&|predicate| exact_parameter_trait(predicate, owner, 2, Some(0)))
        && one(&|predicate| {
            matches!(
                predicate,
                model::Predicate::TypeOutlives {
                    ty: model::Type::Parameter { owner: ty_owner, index: 1 },
                    lifetime: model::Lifetime::Early { owner: lifetime_owner, index: 0 },
                } if ty_owner == owner && lifetime_owner == owner
            )
        })
}

fn exact_text_output(
    output: &model::Type,
    owner: &identity::Definition,
    source: &identity::Definition,
) -> bool {
    matches!(
        output,
        model::Type::Definition { id, arguments }
            if id == source
                && matches!(
                    arguments.as_ref(),
                    [
                        model::Argument::Lifetime(model::Lifetime::Early { owner: lifetime_owner, index: 0 }),
                        model::Argument::Type(model::Type::Parameter { owner: theme_owner, index: 1 }),
                    ] if lifetime_owner == owner && theme_owner == owner
                )
    )
}

fn exact_parameter_trait(
    predicate: &model::Predicate,
    owner: &identity::Definition,
    parameter: u32,
    lifetime: Option<u32>,
) -> bool {
    let model::Predicate::Bound {
        ty: model::Type::Parameter {
            owner: ty_owner,
            index,
        },
        bound:
            model::Bound::Trait {
                trait_id,
                arguments,
                polarity: model::Polarity::Positive,
                binders,
            },
    } = predicate
    else {
        return false;
    };
    ty_owner == owner
        && *index == parameter
        && pinned_unit(trait_id, "iced_core")
        && binders.is_empty()
        && match lifetime {
            None => arguments.is_empty(),
            Some(index) => matches!(
                arguments.as_ref(),
                [model::Argument::Lifetime(model::Lifetime::Early { owner: lifetime_owner, index: actual })]
                    if lifetime_owner == owner && *actual == index
            ),
        }
}

fn select_abi(
    graph: &rawr_import::api::Graph<impl rawr_import::api::Phase>,
    role: AbiRole,
) -> Result<Abi<'_>, profile::Error> {
    let diagnostic = role.diagnostic();
    let mut outcomes = graph.exports().filter(|outcome| {
        let key = outcome.key();
        key.root_alias() == "iced"
            && key.namespace() == identity::Namespace::Type
            && key.path().eq([role.name()])
    });
    let outcome = outcomes.next().ok_or_else(|| foreign(diagnostic))?;
    if outcomes.next().is_some() {
        return Err(canonical(diagnostic));
    }
    let ledger::Path::Imported(export) = outcome else {
        return Err(foreign(diagnostic));
    };
    let definition = exact_definition(graph, export.target())?;
    if !pinned_package(definition, "iced_core")
        || definition
            .display_path()
            .ne(["iced_core", role.module(), role.name()])
        || !matches!(definition.owner(), model::Owner::Free)
        || !matches!(definition.visibility(), model::Visibility::Public)
        || !exact_abi_shape(role, definition)
    {
        return Err(foreign(diagnostic));
    }
    let arguments: Box<[profile::contract::AbiArgument]> = match role {
        AbiRole::Color | AbiRole::Length => Box::default(),
        AbiRole::Rectangle | AbiRole::Vector => {
            Box::from([profile::contract::AbiArgument::DefaultF32 { parameter: 0 }])
        }
    };
    Ok(Abi {
        role,
        export: outcome,
        definition,
        arguments,
    })
}

fn exact_abi_shape(role: AbiRole, definition: &model::Definition) -> bool {
    match (role, definition.kind()) {
        (AbiRole::Color, model::Kind::Struct { fields, generics }) => {
            exact_record_fields(
                fields,
                &[
                    ("r", AbiField::Primitive("f32")),
                    ("g", AbiField::Primitive("f32")),
                    ("b", AbiField::Primitive("f32")),
                    ("a", AbiField::Primitive("f32")),
                ],
            ) && empty_generics(generics)
        }
        (AbiRole::Length, model::Kind::Enum { variants, generics }) => {
            variants.len() == 7
                && exact_variant(&variants[0], "Fit", model::FieldForm::Unit, &[])
                && exact_variant(&variants[1], "Fill", model::FieldForm::Unit, &[])
                && exact_variant(
                    &variants[2],
                    "FillPortion",
                    model::FieldForm::Tuple,
                    &[("0", AbiField::Primitive("u16"))],
                )
                && exact_variant(&variants[3], "Shrink", model::FieldForm::Unit, &[])
                && exact_variant(
                    &variants[4],
                    "Fixed",
                    model::FieldForm::Tuple,
                    &[("0", AbiField::Primitive("f32"))],
                )
                && exact_variant(
                    &variants[5],
                    "Bounded",
                    model::FieldForm::Record,
                    &[("bounds", AbiField::Core), ("sizing", AbiField::Core)],
                )
                && exact_variant(
                    &variants[6],
                    "Fluid",
                    model::FieldForm::Tuple,
                    &[("0", AbiField::Core)],
                )
                && empty_generics(generics)
        }
        (AbiRole::Rectangle, model::Kind::Struct { fields, generics }) => {
            exact_record_fields(
                fields,
                &[
                    ("x", AbiField::Parameter(0)),
                    ("y", AbiField::Parameter(0)),
                    ("width", AbiField::Parameter(0)),
                    ("height", AbiField::Parameter(0)),
                ],
            ) && one_default_f32(definition, generics)
        }
        (AbiRole::Vector, model::Kind::Struct { fields, generics }) => {
            exact_record_fields(
                fields,
                &[("x", AbiField::Parameter(0)), ("y", AbiField::Parameter(0))],
            ) && one_default_f32(definition, generics)
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
enum AbiField<'a> {
    Primitive(&'a str),
    /// A field type defined in the pinned `iced_core` package.
    Core,
    Parameter(u32),
}

fn exact_record_fields(fields: &model::FieldSet, expected: &[(&str, AbiField<'_>)]) -> bool {
    fields.form() == model::FieldForm::Record
        && fields.public_complete()
        && !fields.non_exhaustive()
        && exact_fields(fields.public_fields(), expected)
}

fn exact_variant(
    variant: &model::Variant,
    name: &str,
    form: model::FieldForm,
    expected: &[(&str, AbiField<'_>)],
) -> bool {
    variant.name() == name
        && variant.field_set().form() == form
        && variant.field_set().public_complete()
        && !variant.field_set().non_exhaustive()
        && matches!(variant.discriminant(), model::Default::Absent)
        && exact_fields(variant.field_set().public_fields(), expected)
}

fn exact_fields(fields: &[model::Field], expected: &[(&str, AbiField<'_>)]) -> bool {
    fields.len() == expected.len()
        && fields
            .iter()
            .zip(expected)
            .enumerate()
            .all(|(index, (field, (name, expected)))| {
                field.name() == *name
                    && field.index() == u32::try_from(index).unwrap_or(u32::MAX)
                    && matches!(field.visibility(), model::Visibility::Public)
                    && exact_abi_field(field.ty(), *expected)
            })
}

fn exact_abi_field(ty: &model::Type, expected: AbiField<'_>) -> bool {
    match (ty, expected) {
        (model::Type::Primitive(actual), AbiField::Primitive(expected)) => {
            actual.as_ref() == expected
        }
        (model::Type::Definition { id, arguments }, AbiField::Core) => {
            arguments.is_empty()
                && pinned_unit(id, "iced_core")
                && id.namespace() == identity::Namespace::Type
        }
        (model::Type::Parameter { owner: _, index }, AbiField::Parameter(expected)) => {
            *index == expected
        }
        _ => false,
    }
}

fn empty_generics(generics: &model::Generics) -> bool {
    generics.parameters().len() == 0 && generics.predicates().len() == 0
}

fn one_default_f32(definition: &model::Definition, generics: &model::Generics) -> bool {
    let mut parameters = generics.parameters();
    let Some(parameter) = parameters.next() else {
        return false;
    };
    parameters.next().is_none()
        && generics.predicates().len() == 0
        && parameter.owner() == definition.id()
        && parameter.index() == 0
        && parameter.name() == "T"
        && matches!(
            parameter.kind(),
            model::ParameterKind::Type {
                bounds,
                default: model::Default::Present(model::Type::Primitive(default)),
            } if bounds.is_empty() && default.as_ref() == "f32"
        )
        && match definition.kind() {
            model::Kind::Struct { fields, .. } => fields.public_fields().iter().all(|field| {
                matches!(
                    field.ty(),
                    model::Type::Parameter { owner, index: 0 } if owner == definition.id()
                )
            }),
            _ => false,
        }
}

pub(super) fn diagnosed<T>(
    result: Result<T, profile::Error>,
    stage: Diagnostic,
) -> Result<T, profile::Error> {
    if result.is_err() {
        record_diagnostic(stage);
    }
    result
}

fn foreign(stage: Diagnostic) -> profile::Error {
    record_diagnostic(stage);
    profile::Error::ForeignEvidence
}

fn canonical(stage: Diagnostic) -> profile::Error {
    record_diagnostic(stage);
    profile::Error::Canonical
}

#[cfg(feature = "profile-selection-diagnostic")]
fn reset_diagnostic() {
    SELECTION_DIAGNOSTIC.set(0);
}

#[cfg(not(feature = "profile-selection-diagnostic"))]
const fn reset_diagnostic() {}

#[cfg(feature = "profile-selection-diagnostic")]
fn record_diagnostic(stage: Diagnostic) {
    if SELECTION_DIAGNOSTIC.get() == 0 {
        SELECTION_DIAGNOSTIC.set(stage as u8);
    }
}

#[cfg(not(feature = "profile-selection-diagnostic"))]
const fn record_diagnostic(_stage: Diagnostic) {}

#[cfg(feature = "profile-selection-diagnostic")]
pub(super) fn selection_diagnostic() -> Option<&'static str> {
    match SELECTION_DIAGNOSTIC.get() {
        1 => Some("element-export"),
        2 => Some("element-definition"),
        3 => Some("widget-definition"),
        6 => Some("widget-implementations"),
        8 => Some("pair-binding"),
        9 => Some("target-arguments"),
        10 => Some("contract-evidence"),
        11 => Some("source-export"),
        12 => Some("source-alias"),
        14 => Some("operation-endpoint"),
        15 => Some("color-abi"),
        16 => Some("length-abi"),
        17 => Some("rectangle-abi"),
        18 => Some("vector-abi"),
        _ => None,
    }
}

#[cfg(test)]
mod demand_tests {
    use super::{AbiRole, DEMAND, ELEMENT, OPERATION, SOURCE};

    #[test]
    fn demand_covers_every_lookup() {
        let roles = [
            AbiRole::Color,
            AbiRole::Length,
            AbiRole::Rectangle,
            AbiRole::Vector,
        ];
        let exports = [&ELEMENT[..], &SOURCE[..], &OPERATION[..]]
            .into_iter()
            .map(|path| path.join("::"))
            .chain(roles.iter().map(|role| role.name().to_owned()));
        for path in exports.chain(["Theme", "advanced::Widget"].map(str::to_owned)) {
            let path = format!("iced::{path}");
            assert!(DEMAND.contains(&path.as_str()), "`{path}` is demanded");
        }
        assert!(DEMAND.windows(2).all(|pair| pair[0] < pair[1]));
    }
}

#[cfg(all(test, feature = "profile-selection-diagnostic"))]
mod diagnostic_tests {
    use super::{
        canonical, diagnosed, foreign, reset_diagnostic, selection_diagnostic, Diagnostic,
    };

    #[test]
    fn every_private_selection_stage_has_one_exact_zero_state_tag() {
        for (stage, expected) in [
            (Diagnostic::ElementExport, "element-export"),
            (Diagnostic::ElementDefinition, "element-definition"),
            (Diagnostic::WidgetDefinition, "widget-definition"),
            (Diagnostic::WidgetImplementations, "widget-implementations"),
            (Diagnostic::PairBinding, "pair-binding"),
            (Diagnostic::TargetArguments, "target-arguments"),
            (Diagnostic::ContractEvidence, "contract-evidence"),
            (Diagnostic::SourceExport, "source-export"),
            (Diagnostic::SourceAlias, "source-alias"),
            (Diagnostic::OperationEndpoint, "operation-endpoint"),
        ] {
            reset_diagnostic();
            assert!(selection_diagnostic().is_none());
            assert!(matches!(
                foreign(stage),
                rawr_import::profile::Error::ForeignEvidence
            ));
            assert_eq!(selection_diagnostic(), Some(expected));
            assert!(matches!(
                canonical(stage),
                rawr_import::profile::Error::Canonical
            ));
            assert_eq!(selection_diagnostic(), Some(expected));
        }

        reset_diagnostic();
        let result = diagnosed::<()>(
            Err(foreign(Diagnostic::ElementExport)),
            Diagnostic::ElementDefinition,
        );
        assert!(matches!(
            result,
            Err(rawr_import::profile::Error::ForeignEvidence)
        ));
        assert_eq!(selection_diagnostic(), Some("element-export"));
    }
}

fn exact_definition<'graph>(
    graph: &'graph rawr_import::api::Graph<impl rawr_import::api::Phase>,
    id: &identity::Definition,
) -> Result<&'graph model::Definition, profile::Error> {
    let mut selected = graph
        .definitions()
        .filter(|definition| definition.id() == id);
    let definition = selected.next().ok_or(profile::Error::ForeignEvidence)?;
    if selected.next().is_some() {
        return Err(profile::Error::Canonical);
    }
    Ok(definition)
}

fn exact_alias_arguments(
    alias: &model::Definition,
    alias_generics: &model::Generics,
    target: &model::Definition,
    target_generics: &model::Generics,
    arguments: &[model::Argument],
) -> bool {
    if alias_generics.parameters().len() != arguments.len()
        || target_generics.parameters().len() != arguments.len()
    {
        return false;
    }
    alias_generics
        .parameters()
        .zip(target_generics.parameters())
        .zip(arguments)
        .all(|((alias_parameter, target_parameter), argument)| {
            alias_parameter.owner() == alias.id()
                && target_parameter.owner() == target.id()
                && alias_parameter.index() == target_parameter.index()
                && parameter_kinds_match(alias_parameter.kind(), target_parameter.kind())
                && argument_is_parameter(argument, alias_parameter)
        })
}

fn parameter_kinds_match(left: &model::ParameterKind, right: &model::ParameterKind) -> bool {
    matches!(
        (left, right),
        (
            model::ParameterKind::Lifetime { .. },
            model::ParameterKind::Lifetime { .. }
        ) | (
            model::ParameterKind::Type { .. },
            model::ParameterKind::Type { .. }
        ) | (
            model::ParameterKind::Const { .. },
            model::ParameterKind::Const { .. }
        )
    )
}

fn argument_is_parameter(argument: &model::Argument, parameter: &model::Parameter) -> bool {
    match (argument, parameter.kind()) {
        (
            model::Argument::Lifetime(model::Lifetime::Early { owner, index }),
            model::ParameterKind::Lifetime { .. },
        ) => owner == parameter.owner() && *index == parameter.index(),
        (
            model::Argument::Type(model::Type::Parameter { owner, index }),
            model::ParameterKind::Type { .. },
        ) => owner == parameter.owner() && *index == parameter.index(),
        (
            model::Argument::Const(model::Const::Parameter { owner, index }),
            model::ParameterKind::Const { .. },
        ) => owner == parameter.owner() && *index == parameter.index(),
        _ => false,
    }
}

fn unique_definition<'graph>(
    graph: &'graph rawr_import::api::Graph<impl rawr_import::api::Phase>,
    package: &str,
    suffix: &[&str],
    kind: fn(&model::Definition) -> bool,
) -> Result<&'graph model::Definition, profile::Error> {
    let mut selected = graph.definitions().filter(|definition| {
        let path = definition.display_path();
        let path_len = path.len();
        pinned_package(definition, package)
            && path_len >= suffix.len()
            && path
                .skip(path_len - suffix.len())
                .eq(suffix.iter().copied())
            && kind(definition)
    });
    let definition = selected.next().ok_or(profile::Error::ForeignEvidence)?;
    if selected.next().is_some() {
        return Err(profile::Error::Canonical);
    }
    Ok(definition)
}

fn pinned_package(definition: &model::Definition, name: &str) -> bool {
    pinned_unit(definition.id(), name)
}

/// A definition of the pinned iced revision's package `name`. Compiler
/// identities (StableCrateId, DefPathHash) are not compared: they follow the
/// resolved universe, which the import cache key already covers.
fn pinned_unit(id: &identity::Definition, name: &str) -> bool {
    id.unit().package().is_some_and(|package| {
        package.name() == name
            && package.version() == ICED_VERSION
            && package.source() == ICED_SOURCE
    })
}

fn trait_definition(definition: &model::Definition) -> bool {
    matches!(definition.kind(), model::Kind::Trait { .. })
}

fn implements(
    implementation: &model::Implementation,
    expected: &rawr_import::api::identity::Definition,
) -> bool {
    implementation.polarity() == model::Polarity::Positive
        && matches!(
            implementation.trait_ref(),
            model::Trait::Trait { id, .. } if id == expected
        )
}

fn type_definition(ty: &model::Type) -> Option<&rawr_import::api::identity::Definition> {
    match ty {
        model::Type::Definition { id, .. } => Some(id),
        model::Type::Primitive(_)
        | model::Type::Parameter { .. }
        | model::Type::SelfType(_)
        | model::Type::Tuple(_)
        | model::Type::Slice(_)
        | model::Type::Array { .. }
        | model::Type::Reference { .. }
        | model::Type::RawPointer { .. }
        | model::Type::FunctionPointer(_)
        | model::Type::TraitObject { .. }
        | model::Type::ImplTrait { .. }
        | model::Type::Projection { .. }
        | model::Type::Never => None,
    }
}
