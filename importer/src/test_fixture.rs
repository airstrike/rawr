use crate::api::{self, identity, ledger, model};

const ICED_SOURCE: &str = "git+https://github.com/iced-rs/iced?rev=be88873d38544455e26c7a104eb757da2cbbac6b#be88873d38544455e26c7a104eb757da2cbbac6b";
/// Return a deterministic extracted graph for cross-crate proof tests.
pub fn graph() -> api::Graph<api::Extracted> {
    graph_with_support(false)
}

/// Return classifier evidence with both complete and rejected recursive support.
pub fn support_outcomes() -> api::Graph<api::Extracted> {
    graph_with_support(true)
}

/// Return classifier evidence whose canonical support edge order revisits one provenance.
#[doc(hidden)]
pub fn interleaved_support_outcomes() -> api::Graph<api::Extracted> {
    graph_with_support_order(true, true)
}

/// Return the smallest classifier fixture with one exact implementation ledger
/// and one exact associated outcome.
pub fn classification_implementation_outcomes() -> api::Graph<api::Extracted> {
    let target = definition("a0", identity::Namespace::Type);
    let implementation = definition("a1", identity::Namespace::Type);
    let associated = definition("a2", identity::Namespace::Value);
    let definitions = vec![
        model::Definition::new(
            target.clone(),
            path(&["fixture", "Target"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(model::FieldForm::Unit, Box::default(), true, false),
                generics: generics(),
            },
            span(500, 501),
            model::Origin::Source,
        ),
        model::Definition::new(
            associated.clone(),
            path(&["fixture", "Target", "method"]),
            model::Owner::Implementation(implementation.clone()),
            model::Visibility::Public,
            model::Kind::Function {
                signature: model::Signature::new(
                    model::Safety::Safe,
                    false,
                    false,
                    "\"Rust\"".into(),
                    false,
                    Box::default(),
                    model::Receiver::None,
                    Box::default(),
                    model::Type::Primitive("bool".into()),
                ),
                generics: generics(),
            },
            span(502, 503),
            model::Origin::Source,
        ),
    ];
    let implementations = vec![model::Implementation::new(
        "fixture-target-impl".into(),
        implementation,
        Box::from([target.clone()]),
        model::Trait::Inherent,
        model::Type::Definition {
            id: target.clone(),
            arguments: Box::default(),
        },
        model::Safety::Safe,
        model::Polarity::Positive,
        false,
        false,
        generics(),
        Box::from([associated]),
        span(504, 505),
        model::Origin::Source,
    )];
    let exports = vec![ledger::Path::Imported(ledger::Export::new(
        identity::Export::new(
            "fixture".into(),
            Box::from([Box::<str>::from("Target")]),
            identity::Namespace::Type,
        ),
        target,
        Box::default(),
        Box::default(),
        span(506, 507),
    ))];
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture(),
        Vec::new(),
        definitions,
        exports,
        implementations,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        usize::MAX,
        &mut allocation,
    )
    .expect("implementation-outcome classifier fixture seals")
    .publish(crate::input::proof::Proof::fixture())
    .expect("implementation-outcome freshness proof publishes")
}

/// Return a deterministic graph containing exact pinned iced profile anchors.
pub fn iced_profile() -> api::Graph<api::Extracted> {
    iced_profile_graph(true, IcedElementCase::Primary).expect("pinned iced profile fixture seals")
}

/// Return the pinned iced anchors under one explicitly selected universe.
#[doc(hidden)]
pub fn iced_profile_with_universe(universe: [u8; 32]) -> api::Graph<api::Extracted> {
    iced_profile_graph_with_universe(true, IcedElementCase::Primary, universe)
        .expect("selected iced profile fixture seals")
}

/// Return pinned iced anchors whose `Text` has no `Widget` implementation.
pub fn iced_profile_without_widget_impl() -> api::Graph<api::Extracted> {
    iced_profile_graph(false, IcedElementCase::Primary)
        .expect("pinned iced profile fixture without a Text Widget impl seals")
}

/// Exact public-Element evidence variants for cross-crate profile tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcedElementCase {
    /// The public alias targets the primary Element while its overlay sibling is retained.
    Primary,
    /// The same evidence is constructed in reverse insertion order before canonical sealing.
    Shuffled,
    /// The public Element export is absent.
    MissingAlias,
    /// The public Element export is rejected.
    RejectedAlias,
    /// Two outcomes claim the same public Element export identity.
    DuplicateAlias,
    /// The exported iced definition is not a type alias.
    NonTypeAlias,
    /// The alias normalizes to a non-definition compiler type.
    NonDefinitionTarget,
    /// The alias normalizes to a struct outside the pinned iced source universe.
    ForeignTarget,
    /// The alias normalizes to the distinct overlay Element struct.
    OverlayTarget,
    /// The alias normalizes to a pinned iced_core definition that is not a struct.
    NonStructTarget,
    /// The alias reorders its otherwise exact generic parameter mapping.
    MisorderedArguments,
    /// An anonymous same-spelling import targets the overlay sibling.
    AnonymousSibling,
}

/// Return one exact public-Element fixture outcome.
pub fn iced_profile_element_case(
    case: IcedElementCase,
) -> crate::Result<api::Graph<api::Extracted>> {
    iced_profile_graph(true, case)
}

/// Return an encoded imported facade with the iced semantic anchor types.
pub fn iced_semantic_facade_plan() -> Box<[u8]> {
    crate::surface::facade::iced_semantic_fixture()
}

/// Add ordinary sibling children that force module bindings beside the iced
/// text operation endpoint while preserving the sealed proof rows.
pub fn iced_text_module_facade_plan(bytes: &[u8]) -> Box<[u8]> {
    crate::surface::facade::iced_text_module_fixture(bytes)
}

/// Return an encoded custom-widget facade preceding its iced projection target.
pub fn iced_projection_facade_plan() -> Box<[u8]> {
    crate::surface::facade::iced_projection_fixture()
}

fn iced_profile_graph(
    retain_text_widget: bool,
    case: IcedElementCase,
) -> crate::Result<api::Graph<api::Extracted>> {
    let universe =
        crate::cargo::universe::Catalog::iced_current_v2(&crate::cargo::Limits::standard())?;
    iced_profile_graph_with_universe(retain_text_widget, case, *universe.identity())
}

/// iced master's shapes: `iced::Element` is iced_widget's alias,
/// `Text<'a, Theme>` has no `Renderer`, `text` takes `impl IntoFragment<'a>`,
/// and a widget becomes an Element through `Widget::_boxed`, not `From`.
fn iced_profile_graph_with_universe(
    retain_text_widget: bool,
    case: IcedElementCase,
    universe: [u8; 32],
) -> crate::Result<api::Graph<api::Extracted>> {
    let widget = package_definition("iced_core", "10", identity::Namespace::Type, ICED_SOURCE);
    let element = package_definition("iced_core", "11", identity::Namespace::Type, ICED_SOURCE);
    let dial = package_definition(
        "rawr-iced-dial-fixture",
        "13",
        identity::Namespace::Type,
        "path+file:///dial",
    );
    let button = package_definition("iced_widget", "14", identity::Namespace::Value, ICED_SOURCE);
    let primary = package_definition("iced_widget", "15", identity::Namespace::Value, ICED_SOURCE);
    let widget_impl = package_definition(
        "rawr-iced-dial-fixture",
        "16",
        identity::Namespace::Type,
        "path+file:///dial",
    );
    let element_alias =
        package_definition("iced_widget", "18", identity::Namespace::Type, ICED_SOURCE);
    let overlay = package_definition("iced_core", "19", identity::Namespace::Type, ICED_SOURCE);
    let foreign = package_definition(
        "foreign-element",
        "20",
        identity::Namespace::Type,
        "path+file:///foreign-element",
    );
    let non_struct = package_definition("iced_core", "21", identity::Namespace::Type, ICED_SOURCE);
    let text = package_definition("iced_core", "24", identity::Namespace::Type, ICED_SOURCE);
    let text_alias =
        package_definition("iced_widget", "25", identity::Namespace::Type, ICED_SOURCE);
    let text_widget_impl =
        package_definition("iced_core", "26", identity::Namespace::Type, ICED_SOURCE);
    let text_operation =
        package_definition("iced_widget", "40", identity::Namespace::Value, ICED_SOURCE);
    let theme = package_definition("iced_core", "41", identity::Namespace::Type, ICED_SOURCE);
    let color = package_definition("iced_core", "46", identity::Namespace::Type, ICED_SOURCE);
    let length = package_definition("iced_core", "47", identity::Namespace::Type, ICED_SOURCE);
    let rectangle = package_definition("iced_core", "48", identity::Namespace::Type, ICED_SOURCE);
    let vector = package_definition("iced_core", "49", identity::Namespace::Type, ICED_SOURCE);
    let alias_target = match case {
        IcedElementCase::ForeignTarget => &foreign,
        IcedElementCase::OverlayTarget => &overlay,
        IcedElementCase::NonStructTarget => &non_struct,
        IcedElementCase::Primary
        | IcedElementCase::Shuffled
        | IcedElementCase::MissingAlias
        | IcedElementCase::RejectedAlias
        | IcedElementCase::DuplicateAlias
        | IcedElementCase::NonTypeAlias
        | IcedElementCase::NonDefinitionTarget
        | IcedElementCase::MisorderedArguments
        | IcedElementCase::AnonymousSibling => &element,
    };
    let alias_kind = if case == IcedElementCase::NonTypeAlias {
        model::Kind::Struct {
            fields: model::FieldSet::new(model::FieldForm::Record, Box::default(), false, false),
            generics: element_generics(&element_alias),
        }
    } else {
        let ty = if case == IcedElementCase::NonDefinitionTarget {
            model::Type::Primitive("u8".into())
        } else {
            model::Type::Definition {
                id: alias_target.clone(),
                arguments: element_arguments(
                    &element_alias,
                    case == IcedElementCase::MisorderedArguments,
                ),
            }
        };
        model::Kind::TypeAlias {
            ty,
            generics: element_generics(&element_alias),
        }
    };
    let mut definitions = vec![
        model::Definition::new(
            widget.clone(),
            path(&["iced_core", "widget", "Widget"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Trait {
                unsafe_trait: false,
                auto_trait: false,
                supertraits: Box::default(),
                associated: Box::default(),
                generics: generics(),
            },
            span(300, 310),
            model::Origin::Source,
        ),
        model::Definition::new(
            element.clone(),
            path(&["iced_core", "widget", "element", "Element"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::default(),
                    false,
                    false,
                ),
                generics: element_generics(&element),
            },
            span(311, 320),
            model::Origin::Source,
        ),
        model::Definition::new(
            overlay.clone(),
            path(&["iced_core", "overlay", "element", "Element"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::default(),
                    false,
                    false,
                ),
                generics: element_generics(&overlay),
            },
            span(321, 330),
            model::Origin::Source,
        ),
        model::Definition::new(
            element_alias.clone(),
            path(&["iced_widget", "Element"]),
            model::Owner::Free,
            model::Visibility::Public,
            alias_kind,
            span(331, 340),
            model::Origin::Source,
        ),
        model::Definition::new(
            text.clone(),
            path(&["iced_core", "widget", "text", "Text"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::default(),
                    false,
                    false,
                ),
                generics: text_generics(&text),
            },
            span(331, 332),
            model::Origin::Source,
        ),
        model::Definition::new(
            text_alias.clone(),
            path(&["iced_widget", "text", "Text"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::TypeAlias {
                ty: model::Type::Definition {
                    id: text.clone(),
                    arguments: text_arguments(&text_alias),
                },
                generics: text_generics(&text_alias),
            },
            span(332, 333),
            model::Origin::Source,
        ),
        text_operation_definition(text_operation.clone(), &text),
        model::Definition::new(
            theme.clone(),
            path(&["iced_core", "theme", "Theme"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Enum {
                variants: Box::default(),
                generics: generics(),
            },
            span(333, 334),
            model::Origin::Source,
        ),
        model::Definition::new(
            dial.clone(),
            path(&["dial", "Dial"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::default(),
                    false,
                    false,
                ),
                generics: type_generics(&dial, 0),
            },
            span(351, 360),
            model::Origin::Source,
        ),
        function_definition(
            button.clone(),
            &["iced_widget", "helpers", "button"],
            model::Type::Definition {
                id: element.clone(),
                arguments: Box::default(),
            },
            span(361, 370),
        ),
        function_definition(
            primary.clone(),
            &["iced_widget", "button", "primary"],
            model::Type::Primitive("i64".into()),
            span(371, 380),
        ),
    ];
    definitions.extend(iced_abi_definitions(&color, &length, &rectangle, &vector));
    if case == IcedElementCase::ForeignTarget {
        definitions.push(model::Definition::new(
            foreign.clone(),
            path(&["foreign_element", "Element"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::default(),
                    false,
                    false,
                ),
                generics: element_generics(&foreign),
            },
            span(381, 390),
            model::Origin::Source,
        ));
    }
    if case == IcedElementCase::NonStructTarget {
        definitions.push(model::Definition::new(
            non_struct.clone(),
            path(&["iced_core", "element", "NotElement"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Enum {
                variants: Box::default(),
                generics: element_generics(&non_struct),
            },
            span(391, 400),
            model::Origin::Source,
        ));
    }
    if case == IcedElementCase::Shuffled {
        definitions.reverse();
    }
    definitions.sort_unstable_by(|left, right| left.id().cmp(right.id()));
    let mut implementations = vec![
        model::Implementation::new(
            "dial-widget".into(),
            widget_impl.clone(),
            Box::from([widget.clone(), dial.clone()]),
            model::Trait::Trait {
                id: widget.clone(),
                arguments: Box::default(),
            },
            model::Type::Definition {
                id: dial.clone(),
                arguments: Box::from([model::Argument::Type(model::Type::Parameter {
                    owner: widget_impl.clone(),
                    index: 0,
                })]),
            },
            model::Safety::Safe,
            model::Polarity::Positive,
            false,
            false,
            type_generics(&widget_impl, 0),
            Box::default(),
            span(361, 370),
            model::Origin::Source,
        ),
        model::Implementation::new(
            "text-widget".into(),
            text_widget_impl.clone(),
            Box::from([widget.clone(), text.clone()]),
            model::Trait::Trait {
                id: widget.clone(),
                arguments: Box::default(),
            },
            model::Type::Definition {
                id: text.clone(),
                arguments: text_arguments(&text_widget_impl),
            },
            model::Safety::Safe,
            model::Polarity::Positive,
            false,
            false,
            text_generics(&text_widget_impl),
            Box::default(),
            span(381, 382),
            model::Origin::Source,
        ),
    ];
    if !retain_text_widget {
        implementations.retain(|implementation| implementation.key() != "text-widget");
    }
    if case == IcedElementCase::Shuffled {
        implementations.reverse();
    }
    implementations.sort_unstable_by(|left, right| left.id().cmp(right.id()));
    let mut exports = [
        (dial, "Dial"),
        (button, "widget::button"),
        (primary, "widget::button::primary"),
        (text_alias.clone(), "widget::Text"),
        (text_alias, "widget::text::Text"),
        (text_operation, "widget::text"),
        (text.clone(), "advanced::graphics::core::widget::Text"),
        (text.clone(), "advanced::graphics::core::widget::text::Text"),
        (
            text.clone(),
            "advanced::graphics::futures::core::widget::Text",
        ),
        (
            text.clone(),
            "advanced::graphics::futures::core::widget::text::Text",
        ),
        (text.clone(), "advanced::widget::Text"),
        (text, "advanced::widget::text::Text"),
        (element.clone(), "advanced::graphics::core::Element"),
        (
            element.clone(),
            "advanced::graphics::futures::core::Element",
        ),
        (color, "Color"),
        (length, "Length"),
        (rectangle, "Rectangle"),
        (vector, "Vector"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (target, name))| {
        ledger::Path::Imported(ledger::Export::new(
            identity::Export::new(
                if name == "Dial" { "dial" } else { "iced" }.into(),
                name.split("::")
                    .map(Into::into)
                    .collect::<Vec<Box<str>>>()
                    .into_boxed_slice(),
                target.namespace(),
            ),
            target,
            Box::default(),
            Box::default(),
            span(390 + index as u32, 391 + index as u32),
        ))
    })
    .collect::<Vec<_>>();
    let element_key = || {
        identity::Export::new(
            "iced".into(),
            Box::from([Box::<str>::from("Element")]),
            identity::Namespace::Type,
        )
    };
    match case {
        IcedElementCase::MissingAlias => {}
        IcedElementCase::RejectedAlias => exports.push(ledger::Path::Rejected {
            key: element_key(),
            reason: ledger::Reason::UnresolvedDefinition(element_alias.clone()),
            span: span(401, 402),
        }),
        IcedElementCase::Primary
        | IcedElementCase::Shuffled
        | IcedElementCase::NonTypeAlias
        | IcedElementCase::NonDefinitionTarget
        | IcedElementCase::ForeignTarget
        | IcedElementCase::OverlayTarget
        | IcedElementCase::NonStructTarget
        | IcedElementCase::MisorderedArguments
        | IcedElementCase::AnonymousSibling
        | IcedElementCase::DuplicateAlias => {
            exports.push(ledger::Path::Imported(ledger::Export::new(
                element_key(),
                element_alias.clone(),
                Box::default(),
                Box::default(),
                span(403, 404),
            )));
            if case == IcedElementCase::DuplicateAlias {
                exports.push(ledger::Path::Imported(ledger::Export::new(
                    element_key(),
                    element_alias.clone(),
                    Box::default(),
                    Box::default(),
                    span(405, 406),
                )));
            }
        }
    }
    if case == IcedElementCase::Shuffled {
        exports.reverse();
    }
    exports.sort_unstable_by(|left, right| left.key().cmp(right.key()));
    let anonymous = if case == IcedElementCase::AnonymousSibling {
        vec![ledger::AnonymousImport::new(
            "iced".into(),
            path(&["Element"]),
            overlay,
            Box::from([ledger::Alias::new(
                path(&["iced", "overlay", "Element"]),
                path(&["iced", "Element"]),
                span(407, 408),
            )]),
            Box::from([ledger::Reexport::identified(
                ledger::ReexportKind::Single,
                identity::Reexport::new(element_alias.unit().clone(), "22".into()),
                span(409, 410),
            )]),
            span(411, 412),
        )]
    } else {
        Vec::new()
    };
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture_with_universe(Some(universe)),
        Vec::new(),
        definitions,
        exports,
        implementations,
        Vec::new(),
        Vec::new(),
        anonymous,
        usize::MAX,
        &mut allocation,
    )?
    .publish(crate::input::proof::Proof::fixture())
}

fn iced_abi_definitions(
    color: &identity::Definition,
    length: &identity::Definition,
    rectangle: &identity::Definition,
    vector: &identity::Definition,
) -> Vec<model::Definition> {
    let bounds = package_definition("iced_core", "50", identity::Namespace::Type, ICED_SOURCE);
    let sizing = package_definition("iced_core", "51", identity::Namespace::Type, ICED_SOURCE);
    let fluid = package_definition("iced_core", "52", identity::Namespace::Type, ICED_SOURCE);
    let public_field = |name: &str, index: u32, ty: model::Type| {
        model::Field::new(
            name.into(),
            index,
            model::Visibility::Public,
            ty,
            span(600 + index, 601 + index),
        )
    };
    let variant = |name: &str, form: model::FieldForm, fields: Box<[model::Field]>, index: u32| {
        model::Variant::new(
            name.into(),
            model::FieldSet::new(form, fields, true, false),
            model::Default::Absent,
            span(620 + index, 621 + index),
        )
    };
    vec![
        model::Definition::new(
            color.clone(),
            path(&["iced_core", "color", "Color"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::from([
                        public_field("r", 0, model::Type::Primitive("f32".into())),
                        public_field("g", 1, model::Type::Primitive("f32".into())),
                        public_field("b", 2, model::Type::Primitive("f32".into())),
                        public_field("a", 3, model::Type::Primitive("f32".into())),
                    ]),
                    true,
                    false,
                ),
                generics: generics(),
            },
            span(600, 610),
            model::Origin::Source,
        ),
        model::Definition::new(
            length.clone(),
            path(&["iced_core", "length", "Length"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Enum {
                variants: Box::from([
                    variant("Fit", model::FieldForm::Unit, Box::default(), 0),
                    variant("Fill", model::FieldForm::Unit, Box::default(), 1),
                    variant(
                        "FillPortion",
                        model::FieldForm::Tuple,
                        Box::from([public_field("0", 0, model::Type::Primitive("u16".into()))]),
                        2,
                    ),
                    variant("Shrink", model::FieldForm::Unit, Box::default(), 3),
                    variant(
                        "Fixed",
                        model::FieldForm::Tuple,
                        Box::from([public_field("0", 0, model::Type::Primitive("f32".into()))]),
                        4,
                    ),
                    variant(
                        "Bounded",
                        model::FieldForm::Record,
                        Box::from([
                            public_field(
                                "bounds",
                                0,
                                model::Type::Definition {
                                    id: bounds.clone(),
                                    arguments: Box::default(),
                                },
                            ),
                            public_field(
                                "sizing",
                                1,
                                model::Type::Definition {
                                    id: sizing.clone(),
                                    arguments: Box::default(),
                                },
                            ),
                        ]),
                        5,
                    ),
                    variant(
                        "Fluid",
                        model::FieldForm::Tuple,
                        Box::from([public_field(
                            "0",
                            0,
                            model::Type::Definition {
                                id: fluid.clone(),
                                arguments: Box::default(),
                            },
                        )]),
                        6,
                    ),
                ]),
                generics: generics(),
            },
            span(611, 640),
            model::Origin::Source,
        ),
        abi_generic_struct(
            rectangle,
            &["iced_core", "rectangle", "Rectangle"],
            &["x", "y", "width", "height"],
            641,
        ),
        abi_generic_struct(vector, &["iced_core", "vector", "Vector"], &["x", "y"], 650),
        abi_support_struct(bounds, &["iced_core", "length", "Bounds"], 660),
        abi_support_struct(sizing, &["iced_core", "length", "Sizing"], 670),
        abi_support_struct(fluid, &["iced_core", "length", "Fluid"], 680),
    ]
}

fn abi_generic_struct(
    id: &identity::Definition,
    display: &[&str],
    names: &[&str],
    start: u32,
) -> model::Definition {
    let fields = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            model::Field::new(
                (*name).into(),
                u32::try_from(index).expect("ABI fixture field index fits"),
                model::Visibility::Public,
                model::Type::Parameter {
                    owner: id.clone(),
                    index: 0,
                },
                span(start + index as u32, start + index as u32 + 1),
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    model::Definition::new(
        id.clone(),
        path(display),
        model::Owner::Free,
        model::Visibility::Public,
        model::Kind::Struct {
            fields: model::FieldSet::new(model::FieldForm::Record, fields, true, false),
            generics: model::Generics::new(
                Box::from([model::Parameter::new(
                    id.clone(),
                    0,
                    "T".into(),
                    model::ParameterKind::Type {
                        bounds: Box::default(),
                        default: model::Default::Present(model::Type::Primitive("f32".into())),
                    },
                )]),
                Box::default(),
            ),
        },
        span(start, start + 8),
        model::Origin::Source,
    )
}

fn abi_support_struct(id: identity::Definition, display: &[&str], start: u32) -> model::Definition {
    model::Definition::new(
        id,
        path(display),
        model::Owner::Free,
        model::Visibility::Public,
        model::Kind::Struct {
            fields: model::FieldSet::new(model::FieldForm::Unit, Box::default(), true, false),
            generics: generics(),
        },
        span(start, start + 1),
        model::Origin::Source,
    )
}

fn graph_with_support(retain_support: bool) -> api::Graph<api::Extracted> {
    graph_with_support_order(retain_support, false)
}

fn graph_with_support_order(
    retain_support: bool,
    interleave_provenance: bool,
) -> api::Graph<api::Extracted> {
    captured_graph_with_support_order(retain_support, interleave_provenance)
        .publish(crate::input::proof::Proof::fixture())
        .expect("freshness proof publishes classifier fixture")
}

#[cfg(test)]
pub(crate) fn captured_support_outcomes() -> api::Graph<api::Captured> {
    captured_graph_with_support_order(true, false)
}

fn captured_graph_with_support_order(
    retain_support: bool,
    interleave_provenance: bool,
) -> api::Graph<api::Captured> {
    let pair = definition("01", identity::Namespace::Type);
    let choice = definition("02", identity::Namespace::Type);
    let maximum = definition("03", identity::Namespace::Value);
    let accept = definition("04", identity::Namespace::Value);
    let unsafe_call = definition("05", identity::Namespace::Value);
    let module = definition("06", identity::Namespace::Type);
    let definitions = vec![
        model::Definition::new(
            pair.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("Pair")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::from([
                        model::Field::new(
                            "left".into(),
                            0,
                            model::Visibility::Public,
                            model::Type::Primitive("i64".into()),
                            span(10, 14),
                        ),
                        model::Field::new(
                            "flag".into(),
                            1,
                            model::Visibility::Public,
                            model::Type::Primitive("bool".into()),
                            span(16, 20),
                        ),
                    ]),
                    true,
                    false,
                ),
                generics: generics(),
            },
            span(0, 22),
            model::Origin::Source,
        ),
        model::Definition::new(
            choice.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("Choice")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Enum {
                variants: Box::from([
                    model::Variant::new(
                        "Empty".into(),
                        model::FieldSet::new(model::FieldForm::Unit, Box::default(), true, false),
                        model::Default::Absent,
                        span(30, 35),
                    ),
                    model::Variant::new(
                        "Count".into(),
                        model::FieldSet::new(
                            model::FieldForm::Tuple,
                            Box::from([model::Field::new(
                                "0".into(),
                                0,
                                model::Visibility::Public,
                                model::Type::Primitive("i64".into()),
                                span(37, 40),
                            )]),
                            true,
                            false,
                        ),
                        model::Default::Absent,
                        span(36, 41),
                    ),
                ]),
                generics: generics(),
            },
            span(24, 42),
            model::Origin::Source,
        ),
        model::Definition::new(
            maximum.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("MAX")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Const {
                ty: model::Type::Primitive("i64".into()),
                value: model::Default::Present(model::Const::Literal {
                    ty: "i64".into(),
                    value: "42".into(),
                }),
            },
            span(44, 50),
            model::Origin::Source,
        ),
        function(
            accept.clone(),
            model::Safety::Safe,
            model::Type::Definition {
                id: pair.clone(),
                arguments: Box::default(),
            },
        ),
        function(
            unsafe_call.clone(),
            model::Safety::Unsafe,
            model::Type::Primitive("i64".into()),
        ),
        model::Definition::new(
            module.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("nested")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Module,
            span(80, 86),
            model::Origin::Source,
        ),
    ];
    let support = if retain_support {
        let unresolved = definition("07", identity::Namespace::Type);
        let mut support = vec![
            ledger::Support::Complete {
                id: choice.clone(),
                provenance: pair.clone(),
                role: ledger::SupportRole::Definition,
            },
            ledger::Support::Rejected {
                id: unresolved.clone(),
                provenance: pair.clone(),
                role: ledger::SupportRole::Definition,
                reason: ledger::Reason::UnresolvedDefinition(unresolved),
                span: span(88, 92),
            },
        ];
        if interleave_provenance {
            support.insert(
                1,
                ledger::Support::Complete {
                    id: maximum.clone(),
                    provenance: choice.clone(),
                    role: ledger::SupportRole::Definition,
                },
            );
        }
        support
    } else {
        Vec::new()
    };
    let pair_alias = pair.clone();
    let exports = [
        pair,
        choice,
        maximum,
        accept,
        unsafe_call,
        module,
        pair_alias,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, target)| {
        let namespace = target.namespace();
        ledger::Path::Imported(ledger::Export::new(
            identity::Export::new(
                "fixture".into(),
                Box::from([format!("item_{index}").into_boxed_str()]),
                namespace,
            ),
            target,
            Box::default(),
            Box::default(),
            span(100 + index as u32, 101 + index as u32),
        ))
    })
    .collect();
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture(),
        Vec::new(),
        definitions,
        exports,
        Vec::new(),
        Vec::new(),
        support,
        Vec::new(),
        usize::MAX,
        &mut allocation,
    )
    .expect("canonical classifier fixture seals")
}

/// Return one graph spanning the remaining closed representability families.
pub fn representability_families() -> api::Graph<api::Extracted> {
    let alias = definition("10", identity::Namespace::Type);
    let method = definition("11", identity::Namespace::Value);
    let generic = definition("12", identity::Namespace::Value);
    let definitions = vec![
        model::Definition::new(
            alias.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("Alias")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::TypeAlias {
                ty: model::Type::Primitive("i64".into()),
                generics: generics(),
            },
            span(200, 210),
            model::Origin::Source,
        ),
        model::Definition::new(
            method.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("borrow")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Function {
                signature: model::Signature::new(
                    model::Safety::Safe,
                    false,
                    false,
                    "\"Rust\"".into(),
                    false,
                    Box::default(),
                    model::Receiver::Reference {
                        lifetime: model::Lifetime::Erased,
                        mutability: model::Mutability::Shared,
                        ty: model::Type::Primitive("i64".into()),
                    },
                    Box::default(),
                    model::Type::Primitive("bool".into()),
                ),
                generics: generics(),
            },
            span(212, 230),
            model::Origin::Source,
        ),
        model::Definition::new(
            generic.clone(),
            Box::from([Box::<str>::from("fixture"), Box::<str>::from("generic")]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Function {
                signature: model::Signature::new(
                    model::Safety::Safe,
                    false,
                    false,
                    "\"Rust\"".into(),
                    false,
                    Box::default(),
                    model::Receiver::None,
                    Box::from([model::Type::Parameter {
                        owner: generic.clone(),
                        index: 0,
                    }]),
                    model::Type::Primitive("bool".into()),
                ),
                generics: model::Generics::new(
                    Box::from([model::Parameter::new(
                        generic.clone(),
                        0,
                        "T".into(),
                        model::ParameterKind::Type {
                            bounds: Box::default(),
                            default: model::Default::Absent,
                        },
                    )]),
                    Box::default(),
                ),
            },
            span(232, 250),
            model::Origin::Source,
        ),
    ];
    let mut exports = [alias, method, generic]
        .into_iter()
        .enumerate()
        .map(|(index, target)| {
            ledger::Path::Imported(ledger::Export::new(
                identity::Export::new(
                    "fixture".into(),
                    Box::from([format!("family_{index}").into_boxed_str()]),
                    target.namespace(),
                ),
                target,
                Box::default(),
                Box::default(),
                span(260 + index as u32, 261 + index as u32),
            ))
        })
        .collect::<Vec<_>>();
    exports.push(ledger::Path::Excluded {
        key: identity::Export::new(
            "fixture".into(),
            Box::from([Box::<str>::from("excluded")]),
            identity::Namespace::Value,
        ),
        reason: "fixture policy".into(),
        policy_span: span(270, 271),
        source_span: span(272, 273),
    });
    exports.push(ledger::Path::Rejected {
        key: identity::Export::new(
            "fixture".into(),
            Box::from([Box::<str>::from("rejected")]),
            identity::Namespace::Value,
        ),
        reason: ledger::Reason::InvalidAliasChain,
        span: span(274, 275),
    });
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture(),
        Vec::new(),
        definitions,
        exports,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        usize::MAX,
        &mut allocation,
    )
    .expect("representability-family fixture seals")
    .publish(crate::input::proof::Proof::fixture())
    .expect("freshness proof publishes family fixture")
}

/// A free function taking and returning core's `Option`:
/// `fixture::pick(Option<i64>) -> Option<Option<bool>>`.
pub fn option_function() -> api::Graph<api::Extracted> {
    let option = sysroot_definition("30", identity::Namespace::Type, "core");
    let pick = definition("31", identity::Namespace::Value);
    let of = |ty: model::Type| model::Type::Definition {
        id: option.clone(),
        arguments: Box::from([model::Argument::Type(ty)]),
    };
    let definitions = vec![
        model::Definition::new(
            pick.clone(),
            path(&["fixture", "pick"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Function {
                signature: model::Signature::new(
                    model::Safety::Safe,
                    false,
                    false,
                    "\"Rust\"".into(),
                    false,
                    Box::default(),
                    model::Receiver::None,
                    Box::from([of(model::Type::Primitive("i64".into()))]),
                    of(of(model::Type::Primitive("bool".into()))),
                ),
                generics: generics(),
            },
            span(312, 330),
            model::Origin::Source,
        ),
        model::Definition::new(
            option.clone(),
            path(&["core", "option", "Option"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Sysroot {
                compiler_kind: "enum".into(),
            },
            span(300, 310),
            model::Origin::Source,
        ),
    ];
    let exports = vec![ledger::Path::Imported(ledger::Export::new(
        identity::Export::new(
            "fixture".into(),
            Box::from([Box::<str>::from("pick")]),
            identity::Namespace::Value,
        ),
        pick,
        Box::default(),
        Box::default(),
        span(332, 333),
    ))];
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture(),
        Vec::new(),
        definitions,
        exports,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        usize::MAX,
        &mut allocation,
    )
    .expect("option fixture seals")
    .publish(crate::input::proof::Proof::fixture())
    .expect("freshness proof publishes the option fixture")
}

/// A free function taking and returning core's `Result`:
/// `fixture::check(Result<i64, String>) -> Result<Option<bool>, i64>`.
pub fn result_function() -> api::Graph<api::Extracted> {
    let result = sysroot_definition("40", identity::Namespace::Type, "core");
    let option = sysroot_definition("41", identity::Namespace::Type, "core");
    let string = sysroot_definition("42", identity::Namespace::Type, "alloc");
    let check = definition("43", identity::Namespace::Value);
    let of = |id: &identity::Definition, arguments: Vec<model::Type>| model::Type::Definition {
        id: id.clone(),
        arguments: arguments.into_iter().map(model::Argument::Type).collect(),
    };
    let i64 = || model::Type::Primitive("i64".into());
    let sysroot = |id: &identity::Definition, segments: &[&str], kind: &str, at: u32| {
        model::Definition::new(
            id.clone(),
            path(segments),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Sysroot {
                compiler_kind: kind.into(),
            },
            span(at, at + 10),
            model::Origin::Source,
        )
    };
    let definitions = vec![
        model::Definition::new(
            check.clone(),
            path(&["fixture", "check"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Function {
                signature: model::Signature::new(
                    model::Safety::Safe,
                    false,
                    false,
                    "\"Rust\"".into(),
                    false,
                    Box::default(),
                    model::Receiver::None,
                    Box::from([of(&result, vec![i64(), of(&string, Vec::new())])]),
                    of(
                        &result,
                        vec![
                            of(&option, vec![model::Type::Primitive("bool".into())]),
                            i64(),
                        ],
                    ),
                ),
                generics: generics(),
            },
            span(342, 360),
            model::Origin::Source,
        ),
        sysroot(&result, &["core", "result", "Result"], "enum", 300),
        sysroot(&option, &["core", "option", "Option"], "enum", 311),
        sysroot(&string, &["alloc", "string", "String"], "struct", 322),
    ];
    let mut definitions = definitions;
    definitions.sort_by(|left, right| left.id().cmp(right.id()));
    let exports = vec![ledger::Path::Imported(ledger::Export::new(
        identity::Export::new(
            "fixture".into(),
            Box::from([Box::<str>::from("check")]),
            identity::Namespace::Value,
        ),
        check,
        Box::default(),
        Box::default(),
        span(362, 363),
    ))];
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture(),
        Vec::new(),
        definitions,
        exports,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        usize::MAX,
        &mut allocation,
    )
    .expect("result fixture seals")
    .publish(crate::input::proof::Proof::fixture())
    .expect("freshness proof publishes the result fixture")
}

/// Two free functions generic over `F: Future`, as tokio's
/// `LocalKey::scope` is: `fixture::run<F: Future>(f: F) -> F::Output` and
/// `fixture::scope<F: Future>(f: F) -> Wrap<F>`, whose output names `F`
/// itself.
pub fn future_parameter_functions() -> api::Graph<api::Extracted> {
    let future = sysroot_definition("90", identity::Namespace::Type, "core");
    let output = sysroot_definition("91", identity::Namespace::Type, "core");
    let wrap = definition("92", identity::Namespace::Type);
    let run = definition("93", identity::Namespace::Value);
    let scope = definition("94", identity::Namespace::Value);
    let future_generics = |owner: &identity::Definition| {
        model::Generics::new(
            Box::from([model::Parameter::new(
                owner.clone(),
                0,
                "F".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            )]),
            Box::from([model::Predicate::Bound {
                ty: model::Type::Parameter {
                    owner: owner.clone(),
                    index: 0,
                },
                bound: model::Bound::Trait {
                    trait_id: future.clone(),
                    arguments: Box::default(),
                    polarity: model::Polarity::Positive,
                    binders: Box::default(),
                },
            }]),
        )
    };
    let function = |id: &identity::Definition, name: &str, output: model::Type, at: u32| {
        model::Definition::new(
            id.clone(),
            path(&["fixture", name]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Function {
                signature: model::Signature::new(
                    model::Safety::Safe,
                    false,
                    false,
                    "\"Rust\"".into(),
                    false,
                    Box::default(),
                    model::Receiver::None,
                    Box::from([model::Type::Parameter {
                        owner: id.clone(),
                        index: 0,
                    }]),
                    output,
                ),
                generics: future_generics(id),
            },
            span(at, at + 10),
            model::Origin::Source,
        )
    };
    let sysroot = |id: &identity::Definition, segments: &[&str], kind: &str, at: u32| {
        model::Definition::new(
            id.clone(),
            path(segments),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Sysroot {
                compiler_kind: kind.into(),
            },
            span(at, at + 10),
            model::Origin::Source,
        )
    };
    let mut definitions = vec![
        sysroot(&future, &["core", "future", "Future"], "trait", 600),
        sysroot(
            &output,
            &["core", "future", "Future", "Output"],
            "associated-type",
            611,
        ),
        model::Definition::new(
            wrap.clone(),
            path(&["fixture", "Wrap"]),
            model::Owner::Free,
            model::Visibility::Public,
            model::Kind::Struct {
                fields: model::FieldSet::new(
                    model::FieldForm::Record,
                    Box::default(),
                    false,
                    false,
                ),
                generics: type_generics(&wrap, 0),
            },
            span(622, 632),
            model::Origin::Source,
        ),
        function(
            &run,
            "run",
            model::Type::Projection {
                trait_id: future.clone(),
                associated: output.clone(),
                arguments: Box::from([model::Argument::Type(model::Type::Parameter {
                    owner: run.clone(),
                    index: 0,
                })]),
            },
            633,
        ),
        function(
            &scope,
            "scope",
            model::Type::Definition {
                id: wrap.clone(),
                arguments: Box::from([model::Argument::Type(model::Type::Parameter {
                    owner: scope.clone(),
                    index: 0,
                })]),
            },
            644,
        ),
    ];
    definitions.sort_by(|left, right| left.id().cmp(right.id()));
    let export = |name: &str, namespace, id: &identity::Definition, at: u32| {
        ledger::Path::Imported(ledger::Export::new(
            identity::Export::new(
                "fixture".into(),
                Box::from([Box::<str>::from(name)]),
                namespace,
            ),
            id.clone(),
            Box::default(),
            Box::default(),
            span(at, at + 1),
        ))
    };
    let exports = vec![
        export("Wrap", identity::Namespace::Type, &wrap, 653),
        export("run", identity::Namespace::Value, &run, 655),
        export("scope", identity::Namespace::Value, &scope, 657),
    ];
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture(),
        Vec::new(),
        definitions,
        exports,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        usize::MAX,
        &mut allocation,
    )
    .expect("future parameter fixture seals")
    .publish(crate::input::proof::Proof::fixture())
    .expect("freshness proof publishes the future parameter fixture")
}

/// Return the generic free function from the family fixture as an installable
/// facade, without the classifier-only primitive receiver method.
pub fn representability_generic() -> api::Graph<api::Extracted> {
    let graph = representability_families();
    let generic = definition("12", identity::Namespace::Value);
    let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
    api::Graph::seal(
        &api::Context::fixture(),
        Vec::new(),
        graph
            .definitions()
            .filter(|definition| definition.id() == &generic)
            .cloned()
            .collect(),
        graph
            .exports()
            .filter(|export| export.key().path().eq(["family_2"]))
            .cloned()
            .collect(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        usize::MAX,
        &mut allocation,
    )
    .expect("generic free-function fixture seals")
    .publish(crate::input::proof::Proof::fixture())
    .expect("freshness proof publishes generic fixture")
}

fn function(
    id: identity::Definition,
    safety: model::Safety,
    input: model::Type,
) -> model::Definition {
    model::Definition::new(
        id,
        Box::from([Box::<str>::from("fixture"), Box::<str>::from("call")]),
        model::Owner::Free,
        model::Visibility::Public,
        model::Kind::Function {
            signature: model::Signature::new(
                safety,
                false,
                false,
                "\"Rust\"".into(),
                false,
                Box::default(),
                model::Receiver::None,
                Box::from([input]),
                model::Type::Primitive("bool".into()),
            ),
            generics: generics(),
        },
        span(60, 78),
        model::Origin::Source,
    )
}

fn function_definition(
    id: identity::Definition,
    display_path: &[&str],
    output: model::Type,
    source: model::Span,
) -> model::Definition {
    model::Definition::new(
        id,
        path(display_path),
        model::Owner::Free,
        model::Visibility::Public,
        model::Kind::Function {
            signature: model::Signature::new(
                model::Safety::Safe,
                false,
                false,
                "\"Rust\"".into(),
                false,
                Box::default(),
                model::Receiver::None,
                Box::default(),
                output,
            ),
            generics: generics(),
        },
        source,
        model::Origin::Source,
    )
}

fn text_operation_definition(
    id: identity::Definition,
    text: &identity::Definition,
) -> model::Definition {
    let lifetime = model::Lifetime::Early {
        owner: id.clone(),
        index: 0,
    };
    let parameter = |index| model::Type::Parameter {
        owner: id.clone(),
        index,
    };
    let trait_predicate =
        |parameter: u32, hash: &str, arguments: Box<[model::Argument]>| model::Predicate::Bound {
            ty: model::Type::Parameter {
                owner: id.clone(),
                index: parameter,
            },
            bound: model::Bound::Trait {
                trait_id: package_definition(
                    "iced_core",
                    hash,
                    identity::Namespace::Type,
                    ICED_SOURCE,
                ),
                arguments,
                polarity: model::Polarity::Positive,
                binders: Box::default(),
            },
        };
    let generics = model::Generics::new(
        Box::from([
            model::Parameter::new(
                id.clone(),
                0,
                "'a".into(),
                model::ParameterKind::Lifetime {
                    bounds: Box::default(),
                },
            ),
            model::Parameter::new(
                id.clone(),
                1,
                "Theme".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ),
            model::Parameter::new(
                id.clone(),
                2,
                "impl IntoFragment<'a>".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ),
        ]),
        Box::from([
            trait_predicate(1, "53", Box::default()),
            trait_predicate(
                2,
                "55",
                Box::from([model::Argument::Lifetime(lifetime.clone())]),
            ),
            model::Predicate::TypeOutlives {
                ty: parameter(1),
                lifetime: lifetime.clone(),
            },
        ]),
    );
    model::Definition::new(
        id.clone(),
        path(&["iced_widget", "helpers", "text"]),
        model::Owner::Free,
        model::Visibility::Public,
        model::Kind::Function {
            signature: model::Signature::new(
                model::Safety::Safe,
                false,
                false,
                "\"Rust\"".into(),
                false,
                Box::default(),
                model::Receiver::None,
                Box::from([parameter(2)]),
                model::Type::Definition {
                    id: text.clone(),
                    arguments: Box::from([
                        model::Argument::Lifetime(lifetime),
                        model::Argument::Type(parameter(1)),
                    ]),
                },
            ),
            generics,
        },
        span(335, 336),
        model::Origin::Source,
    )
}

fn definition(hash: &str, namespace: identity::Namespace) -> identity::Definition {
    package_definition("fixture", hash, namespace, "path+file:///fixture")
}

fn package_definition(
    package: &str,
    hash: &str,
    namespace: identity::Namespace,
    source: &str,
) -> identity::Definition {
    let version = if source == ICED_SOURCE {
        "0.15.0-dev"
    } else {
        "0.1.0"
    };
    identity::Definition::new(
        identity::Unit::new(
            identity::Authority::Cargo(identity::Package::new(
                format!("{source}#{package}@{version}").into_boxed_str(),
                package.into(),
                version.into(),
                source.into(),
            )),
            "fixture-target".into(),
            "lib".into(),
            package.replace('-', "_").into_boxed_str(),
            identity::Compilation::Target,
            Box::default(),
            "0000000000000001".into(),
        ),
        hash.into(),
        namespace,
    )
}

fn sysroot_definition(
    hash: &str,
    namespace: identity::Namespace,
    crate_name: &str,
) -> identity::Definition {
    identity::Definition::new(
        identity::Unit::new(
            identity::Authority::Sysroot(identity::Sysroot::new(
                "9e091e6fd37c34b05f1bc4099a208921b4bb11ba".into(),
                "aarch64-apple-darwin".into(),
                crate_name.into(),
                "0000000000000002".into(),
            )),
            "aarch64-apple-darwin".into(),
            "lib".into(),
            crate_name.into(),
            identity::Compilation::Target,
            Box::default(),
            "0000000000000002".into(),
        ),
        hash.into(),
        namespace,
    )
}

fn generics() -> model::Generics {
    model::Generics::new(Box::default(), Box::default())
}

fn type_generics(owner: &identity::Definition, index: u32) -> model::Generics {
    model::Generics::new(
        Box::from([model::Parameter::new(
            owner.clone(),
            index,
            "T".into(),
            model::ParameterKind::Type {
                bounds: Box::default(),
                default: model::Default::Absent,
            },
        )]),
        Box::default(),
    )
}

fn element_generics(owner: &identity::Definition) -> model::Generics {
    model::Generics::new(
        Box::from([
            model::Parameter::new(
                owner.clone(),
                0,
                "'a".into(),
                model::ParameterKind::Lifetime {
                    bounds: Box::default(),
                },
            ),
            model::Parameter::new(
                owner.clone(),
                1,
                "Message".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ),
            model::Parameter::new(
                owner.clone(),
                2,
                "Theme".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ),
            model::Parameter::new(
                owner.clone(),
                3,
                "Renderer".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ),
        ]),
        Box::default(),
    )
}

fn element_arguments(owner: &identity::Definition, misordered: bool) -> Box<[model::Argument]> {
    let mut arguments = vec![
        model::Argument::Lifetime(model::Lifetime::Early {
            owner: owner.clone(),
            index: 0,
        }),
        model::Argument::Type(model::Type::Parameter {
            owner: owner.clone(),
            index: 1,
        }),
        model::Argument::Type(model::Type::Parameter {
            owner: owner.clone(),
            index: 2,
        }),
        model::Argument::Type(model::Type::Parameter {
            owner: owner.clone(),
            index: 3,
        }),
    ];
    if misordered {
        arguments.swap(1, 2);
    }
    arguments.into_boxed_slice()
}

fn text_generics(owner: &identity::Definition) -> model::Generics {
    model::Generics::new(
        Box::from([
            model::Parameter::new(
                owner.clone(),
                0,
                "'a".into(),
                model::ParameterKind::Lifetime {
                    bounds: Box::default(),
                },
            ),
            model::Parameter::new(
                owner.clone(),
                1,
                "Theme".into(),
                model::ParameterKind::Type {
                    bounds: Box::default(),
                    default: model::Default::Absent,
                },
            ),
        ]),
        Box::default(),
    )
}

fn text_arguments(owner: &identity::Definition) -> Box<[model::Argument]> {
    Box::from([
        model::Argument::Lifetime(model::Lifetime::Early {
            owner: owner.clone(),
            index: 0,
        }),
        model::Argument::Type(model::Type::Parameter {
            owner: owner.clone(),
            index: 1,
        }),
    ])
}

fn span(start: u32, end: u32) -> model::Span {
    model::Span::new("fixture/src/lib.rs".into(), start, end, 1, start, 1, end)
}

fn path(segments: &[&str]) -> Box<[Box<str>]> {
    segments
        .iter()
        .map(|segment| Box::<str>::from(*segment))
        .collect::<Vec<_>>()
        .into_boxed_slice()
}
