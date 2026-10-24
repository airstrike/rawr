//! The iced application facade for Rawr.
//!
//! Compiler metadata and the origin/ABI-proven native application boundary
//! live together here. Widget descriptors and task lowering remain separate
//! layers over this boundary.

// Public facade and snapshot APIs retain their structured, source-bearing error
// enums. Boxing their `Err` values would change those proven diagnostic APIs.
#![allow(clippy::result_large_err)]

pub mod application;
pub mod control;

pub mod diagnostic;
mod dispatch;
mod effect;
mod marker;
pub mod native;
pub mod preview;
pub mod profile;
pub mod program;
pub mod reload;
mod renderer;
mod screenshot;
pub mod runtime;
pub mod snapshot;
pub mod subscription;
pub mod task;
#[cfg(test)]
mod task_thread_tests;
mod io_facade;
mod path_facade;
pub mod theme;
mod tokio_facade;
pub mod widget;

/// What brings a program to rawr-iced, as data: the manifest the
/// `rawr-iced` binary prints for `rawr`, and the prelude a snippet host
/// imports (see `manifest.toml`).
pub const MANIFEST: &str = include_str!("../manifest.toml");

use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{capability, receiver, Engine};

const OWNED_VALUE_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Copy,
    capability::Kind::PartialEq,
    capability::Kind::Send,
    capability::Kind::Static,
];
const OWNED_DEFAULT_VALUE_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Default,
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Copy,
    capability::Kind::PartialEq,
    capability::Kind::Send,
    capability::Kind::Static,
];
/// A `Clone` value that is not `Copy`, as iced's `Event`.
const DEBUG_VALUE_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::PartialEq,
    capability::Kind::Send,
    capability::Kind::Static,
];
const DEBUG_EQ_VALUE_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];
const OWNED_EQ_VALUE_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Copy,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];
const OWNED_EQ_DEFAULT_VALUE_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Default,
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Copy,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// `{:?}` of the facade's value types prints Rust's `Debug` text.
fn register_debug() {
    use rawr::host::debug::register;
    tokio_facade::register_debug();
    path_facade::register_debug();
    io_facade::register_debug();
    // `{}` of a facade type with `to_string` prints its Rust `Display`.
    rawr::host::display::register::<iced::ContentFit>();
    rawr::host::display::register::<iced::font::Family>();
    register::<iced::Border>();
    register::<iced::Color>();
    register::<iced::ContentFit>();
    register::<iced::event::Status>();
    register::<iced::widget::scrollable::Anchor>();
    register::<iced::widget::scrollable::Direction>();
    register::<iced::widget::scrollable::Scroll>();
    register::<iced::widget::scrollable::Scrollbar>();
    register::<iced::widget::scrollable::Source>();
    register::<iced::widget::scrollable::Viewport>();
    register::<marker::ScrollBoth>();
    register::<iced::Degrees>();
    register::<iced::Rotation>();
    register::<iced::Radians>();
    register::<iced::widget::checkbox::Icon>();
    register::<iced::widget::text::LineHeight>();
    register::<iced::widget::text::Shaping>();
    register::<iced::widget::image::FilterMethod>();
    register::<iced::widget::image::Handle>();
    register::<iced::widget::svg::Handle>();
    register::<iced::Event>();
    register::<task::Handle>();
    register::<iced::Font>();
    register::<iced::keyboard::Event>();
    register::<iced::keyboard::Key>();
    register::<iced::keyboard::key::Named>();
    register::<iced::keyboard::Modifiers>();
    register::<iced::Length>();
    register::<iced::mouse::Button>();
    register::<iced::mouse::Event>();
    register::<iced::mouse::Interaction>();
    register::<iced::Padding>();
    register::<iced::Point>();
    register::<iced::Rectangle>();
    register::<iced::Size>();
    register::<iced::touch::Event>();
    register::<iced::Vector>();
    register::<iced::window::Event>();
    register::<iced::window::Id>();
    register::<std::time::Duration>();
    register::<iced::mouse::Cursor>();
    register::<iced::widget::canvas::Path>();
    register::<iced::widget::canvas::Stroke<'static>>();
    register::<iced::widget::canvas::Fill>();
    register::<iced::widget::canvas::Style>();
    register::<iced::widget::canvas::fill::Rule>();
    register::<iced::widget::canvas::LineCap>();
    register::<iced::widget::canvas::LineJoin>();
    register::<iced::widget::canvas::Text>();
    register::<iced::widget::text::Alignment>();
    register::<marker::Cache>();
    register::<marker::Geometry>();
}

/// Build the validated compile-time `iced` facade contract.
pub fn facade() -> Result<facade::Definition, host::Error> {
    register_debug();
    let mut builder = facade::Builder::new("iced", facade::Abi::new(16)?)?;
    builder.module("alignment")?;
    builder.module("animation")?;
    builder.module("border")?;
    builder.module("font")?;
    builder.module("mouse")?;
    builder.module("padding")?;
    builder.module("theme")?;
    builder.module("theme::palette")?;
    builder.module("time")?;
    builder.module("task")?;
    builder.module("subscription")?;
    builder.module("widget")?;
    builder.module("widget::button")?;
    builder.module("widget::checkbox")?;
    builder.module("widget::combo_box")?;
    builder.module("widget::component")?;
    builder.module("widget::container")?;
    builder.module("widget::progress_bar")?;
    builder.module("widget::float")?;
    builder.module("widget::radio")?;
    builder.module("widget::rule")?;
    builder.module("widget::image")?;
    builder.module("widget::svg")?;
    builder.module("widget::qr_code")?;
    builder.module("widget::operation")?;
    builder.module("widget::selector")?;
    builder.module("widget::operation::scrollable")?;
    builder.module("window")?;
    builder.module("futures")?;
    builder.module("futures::future")?;
    builder.module("event")?;
    builder.module("keyboard")?;
    builder.module("keyboard::key")?;
    builder.module("touch")?;
    builder.module("widget::scrollable")?;
    builder.module("widget::slider")?;
    builder.module("widget::table")?;
    builder.module("widget::text")?;
    builder.module("widget::text_input")?;
    builder.module("widget::toggler")?;
    builder.module("widget::tooltip")?;

    builder.type_::<marker::Result>("Result", &[], &[])?;
    builder.required_type::<task::Plan>("Task", &["Message"], &[])?;
    // A Rust `impl Sipper<Output, Progress>`, and iced's `task::Handle`.
    builder.type_::<task::Sip>("task::Sipper", &["Output", "Progress"], &[])?;
    // The `task::Sender` a script sipper's builder sends its progress with.
    builder.type_::<task::Sender>("task::Sender", &["Progress"], &[])?;
    builder.type_::<task::Handle>(
        "task::Handle",
        &[],
        &[capability::Kind::Debug, capability::Kind::Clone],
    )?;
    builder.stream_plan_type("Subscription", "Message")?;
    builder.value_type::<theme::Value>(
        "Theme",
        &[],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.value_type::<iced::theme::Palette>("theme::Palette", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::theme::palette::Background>(
        "theme::palette::Background",
        &[],
        OWNED_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::theme::palette::Swatch>(
        "theme::palette::Swatch",
        &[],
        OWNED_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::theme::palette::Pair>(
        "theme::palette::Pair",
        &[],
        OWNED_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::theme::palette::Seed>(
        "theme::palette::Seed",
        &[],
        OWNED_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<marker::Alignment>(
        "Alignment",
        &[],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.private_type::<marker::Application>("Application", &["State", "Message"], &[])?;
    builder.private_type::<marker::container::Style>("widget::container::Builtin", &[], &[])?;
    builder.default_value_type::<iced::widget::container::Style>(
        "widget::container::Style",
        &[],
        OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::Border>("Border", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.private_type::<marker::text::Style>("widget::text::Style", &[], &[])?;
    builder.default_value_type::<iced::Color>("Color", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder
        .carried::<theme::CarryColor>()
        .carried::<theme::CarryTheme>();
    builder.value_type::<iced::Length>("Length", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::Padding>("Padding", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::Font>("Font", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::Point>("Point", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::Rectangle>("Rectangle", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::Size>("Size", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::Vector>("Vector", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::Rotation>("Rotation", &[], OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    for path in ["Degrees", "Radians"] {
        let capabilities = &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Send,
            capability::Kind::Static,
        ];
        match path {
            "Degrees" => builder.value_type::<iced::Degrees>(path, &[], capabilities)?,
            _ => builder.value_type::<iced::Radians>(path, &[], capabilities)?,
        };
    }
    builder.default_value_type::<iced::mouse::Interaction>(
        "mouse::Interaction",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.type_::<widget::Value>("Element", &["Message"], &[])?;
    // `impl Widget<Message>` is an Element: every widget converts into one.
    for path in ["widget::Widget", "Widget"] {
        builder.trait_::<marker::Widget>(path, &["Message"])?;
        builder.project(path, "Element", &["Message"])?;
    }
    builder.trait_::<marker::Function>("Function", &[])?;
    builder.type_::<marker::Column>("widget::Column", &["Message"], &[])?;
    builder.type_::<marker::Row>("widget::Row", &["Message"], &[])?;
    builder.type_::<marker::Container>("widget::Container", &["Message"], &[])?;
    builder.type_::<marker::Button>("widget::Button", &["Message"], &[])?;
    builder.type_::<marker::Text>("widget::Text", &[], &[])?;
    builder.type_::<marker::TextInput>("widget::TextInput", &["Message"], &[])?;
    builder.type_::<marker::Checkbox>("widget::Checkbox", &["Message"], &[])?;
    builder.type_::<marker::Toggler>("widget::Toggler", &["Message"], &[])?;
    builder.type_::<marker::Slider>("widget::Slider", &["T", "Message"], &[])?;
    builder.type_::<marker::PickList>("widget::PickList", &["T", "Message"], &[])?;
    builder.type_::<marker::ComboBoxState>("widget::combo_box::State", &["T"], &[])?;
    builder.type_::<marker::ComboBox>("widget::ComboBox", &["T", "Message"], &[])?;
    builder.type_::<marker::Radio>("widget::Radio", &["Message"], &[])?;
    builder.type_::<marker::Scrollable>("widget::Scrollable", &["Message"], &[])?;
    builder.type_::<marker::Space>("widget::Space", &[], &[])?;
    builder.type_::<marker::Logo>("widget::Logo", &[], &[])?;
    builder.type_::<marker::ProgressBar>("widget::ProgressBar", &[], &[])?;
    builder.type_::<marker::Transition>("widget::Transition", &["Message"], &[])?;
    builder.type_::<marker::Tooltip>("widget::Tooltip", &["Message"], &[])?;
    builder.type_::<marker::Sensor>("widget::Sensor", &["Message"], &[])?;
    builder.type_::<marker::Renderer>("widget::Renderer", &[], &[])?;
    // iced's `component::Component`, which a script type implements; it
    // converts into an `Instance` that `component(..)` makes a widget.
    builder.private_type::<marker::Component>("widget::component::Instance", &["Message"], &[])?;
    builder.implementable_trait::<marker::ComponentTrait>(
        "widget::component::Component",
        &["Message"],
        COMPONENT,
    )?;
    builder.project("widget::component::Component", "widget::component::Instance", &["Message"])?;
    // iced's `pub use component::Component` in `widget`, and its
    // `iced::Renderer`.
    builder.alias("widget::Component", "widget::component::Component")?;
    builder.alias("Renderer", "widget::Renderer")?;
    builder.default_value_type::<iced::widget::tooltip::Position>(
        "widget::tooltip::Position",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<marker::Horizontal>("alignment::Horizontal", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.value_type::<marker::Vertical>("alignment::Vertical", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::widget::selector::Target>(
        "widget::selector::Target",
        &[],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.value_type::<iced::widget::Id>(
        "widget::Id",
        &[],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.default_value_type::<iced::widget::operation::Animation>(
        "widget::operation::Animation",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::widget::scrollable::RelativeOffset>(
        "widget::scrollable::RelativeOffset",
        &[],
        OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::widget::scrollable::AbsoluteOffset>(
        "widget::scrollable::AbsoluteOffset",
        &[],
        OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::widget::scrollable::Anchor>(
        "widget::scrollable::Anchor",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::widget::scrollable::Scrollbar>(
        "widget::scrollable::Scrollbar",
        &[],
        OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::widget::scrollable::Direction>(
        "widget::scrollable::Direction",
        &[],
        OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<marker::ScrollBoth>(
        "widget::scrollable::Direction::Both",
        &[],
        OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::widget::scrollable::Source>(
        "widget::scrollable::Source",
        &[],
        OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    for path in ["widget::scrollable::Scroll", "widget::scrollable::Viewport"] {
        let capabilities = &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::Send,
            capability::Kind::Static,
        ];
        match path {
            "widget::scrollable::Scroll" => {
                builder.type_::<iced::widget::scrollable::Scroll>(path, &[], capabilities)?
            }
            _ => builder.type_::<iced::widget::scrollable::Viewport>(path, &[], capabilities)?,
        };
    }
    builder.value_type::<iced::Event>("Event", &[], DEBUG_VALUE_CAPABILITIES)?;
    builder.alias("event::Event", "Event")?;
    builder.value_type::<iced::window::Event>("window::Event", &[], DEBUG_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::mouse::Event>("mouse::Event", &[], DEBUG_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::mouse::Button>("mouse::Button", &[], OWNED_EQ_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::touch::Event>("touch::Event", &[], DEBUG_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::keyboard::Event>("keyboard::Event", &[], DEBUG_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::keyboard::Key>("keyboard::Key", &[], DEBUG_EQ_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::keyboard::key::Named>(
        "keyboard::key::Named",
        &[],
        OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::keyboard::Modifiers>(
        "keyboard::Modifiers",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::window::Id>("window::Id", &[], OWNED_EQ_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::event::Status>("event::Status", &[], OWNED_EQ_VALUE_CAPABILITIES)?;
    builder.type_::<marker::Stack>("widget::Stack", &["Message"], &[])?;
    builder.project("widget::Stack", "Element", &["Message"])?;
    builder.value_type::<iced::widget::image::Handle>(
        "widget::image::Handle",
        &[],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.value_type::<iced::widget::svg::Handle>(
        "widget::svg::Handle",
        &[],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.default_value_type::<iced::ContentFit>("ContentFit", &[], OWNED_EQ_DEFAULT_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::widget::text::Shaping>(
        "widget::text::Shaping",
        &[],
        OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::widget::text::LineHeight>(
        "widget::text::LineHeight",
        &[],
        OWNED_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::widget::checkbox::Icon>(
        "widget::checkbox::Icon",
        &[],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.default_value_type::<iced::widget::image::FilterMethod>(
        "widget::image::FilterMethod",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<marker::QrData>(
        "widget::qr_code::Data",
        &[],
        &[capability::Kind::Debug, capability::Kind::Clone, capability::Kind::PartialEq, capability::Kind::Send, capability::Kind::Static],
    )?;
    builder.value_type::<iced::widget::qr_code::Error>(
        "widget::qr_code::Error",
        &[],
        OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.type_::<marker::Image>("widget::Image", &[], &[])?;
    builder.project_with("widget::Image", "Element", &[host::type_def::Argument::fresh()])?;
    builder.type_::<marker::Svg>("widget::Svg", &[], &[])?;
    builder.project_with("widget::Svg", "Element", &[host::type_def::Argument::fresh()])?;
    builder.type_::<marker::QrCode>("widget::QRCode", &[], &[])?;
    builder.type_::<marker::Span>("widget::text::Span", &["Link"], &[])?;
    builder.type_::<marker::Rich>("widget::text::Rich", &["Link", "Message"], &[])?;
    builder.type_::<marker::TableColumn>("widget::table::Column", &["T", "Message"], &[])?;
    builder.type_::<marker::Table>("widget::Table", &["Message"], &[])?;
    builder.project("widget::Table", "Element", &["Message"])?;
    builder.project("widget::text::Rich", "Element", &["Message"])?;
    builder.project_with("widget::QRCode", "Element", &[host::type_def::Argument::fresh()])?;
    builder.type_::<marker::Pin>("widget::Pin", &["Message"], &[])?;
    builder.project("widget::Pin", "Element", &["Message"])?;
    builder.type_::<marker::Float>("widget::Float", &["Message"], &[])?;
    builder.project("widget::Float", "Element", &["Message"])?;
    builder.type_::<marker::MouseArea>("widget::MouseArea", &["Message"], &[])?;
    builder.project("widget::MouseArea", "Element", &["Message"])?;
    builder.default_value_type::<iced::font::Weight>(
        "font::Weight",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::font::Stretch>(
        "font::Stretch",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<iced::font::Style>(
        "font::Style",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::font::Error>("font::Error", &[], OWNED_EQ_VALUE_CAPABILITIES)?;
    builder.default_value_type::<iced::font::Family>(
        "font::Family",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.default_value_type::<std::time::Duration>(
        "time::Duration",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<marker::Animation>(
        "Animation",
        &["T"],
        &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.value_type::<marker::Easing>("animation::Easing", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.value_type::<marker::Instant>("time::Instant", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.project("widget::Column", "Element", &["Message"])?;
    builder.project("widget::Row", "Element", &["Message"])?;
    builder.project("widget::Container", "Element", &["Message"])?;
    builder.project("widget::Button", "Element", &["Message"])?;
    builder.project_with(
        "widget::Text",
        "Element",
        &[host::type_def::Argument::fresh()],
    )?;
    builder.project("widget::TextInput", "Element", &["Message"])?;
    builder.project("widget::Checkbox", "Element", &["Message"])?;
    builder.project("widget::Toggler", "Element", &["Message"])?;
    builder.project("widget::Slider", "Element", &["Message"])?;
    builder.project("widget::PickList", "Element", &["Message"])?;
    builder.project("widget::ComboBox", "Element", &["Message"])?;
    builder.project("widget::Radio", "Element", &["Message"])?;
    builder.project("widget::Scrollable", "Element", &["Message"])?;
    builder.project_with(
        "widget::Space",
        "Element",
        &[host::type_def::Argument::fresh()],
    )?;
    builder.project_with(
        "widget::Logo",
        "Element",
        &[host::type_def::Argument::fresh()],
    )?;
    builder.project_with(
        "widget::ProgressBar",
        "Element",
        &[host::type_def::Argument::fresh()],
    )?;
    builder.project("widget::Transition", "Element", &["Message"])?;
    builder.project("widget::Tooltip", "Element", &["Message"])?;
    builder.project("widget::Sensor", "Element", &["Message"])?;

    builder.item(item::Definition::value(
        "Center",
        type_ref::Type::registered("Alignment")?,
    )?)?;
    for name in ["Start", "Center", "End"] {
        builder.item(item::Definition::associated_value(
            format!("Alignment::{name}"),
            type_ref::Type::registered("Alignment")?,
        )?)?;
    }
    for name in ["Fill", "Fit", "Shrink"] {
        builder.item(item::Definition::value(
            name,
            type_ref::Type::registered("Length")?,
        )?)?;
    }
    builder.item(item::Definition::function(
        "FillPortion",
        integer_signature("Length", 1)?,
    )?)?;
    for name in BUILT_IN_THEMES {
        builder.item(item::Definition::associated_value(
            format!("Theme::{name}"),
            type_ref::Type::registered("Theme")?,
        )?)?;
    }
    // `theme.to_string()`: its name, as iced's `Display`.
    let mut name = signature::Builder::new();
    name.output(type_ref::Type::builtin(type_ref::Builtin::String));
    builder.item(item::Definition::method("Theme::to_string", receiver::Mode::Shared, name.finish())?)?;
    // iced's `Theme::ALL`, every built-in theme.
    builder.item(item::Definition::associated_value(
        "Theme::ALL",
        type_ref::Type::slice(type_ref::Type::registered("Theme")?),
    )?)?;
    builder.item(item::Definition::intrinsic_function(
        "run",
        run_signature()?,
    )?)?;
    builder.item(item::Definition::intrinsic_function(
        "application",
        application_signature()?,
    )?)?;
    builder.item(
        item::Definition::associated_function("Task::none", task_none_signature()?)?
            .discardable()
            .elidable(),
    )?;
    builder.item(item::Definition::associated_function(
        "Task::done",
        task_done_signature()?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "Task::perform",
        task_perform_signature()?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "Task::future",
        task_future_signature()?,
    )?)?;
    builder.item(item::Definition::method(
        "Task::discard",
        receiver::Mode::Owned,
        task_discard_signature()?,
    )?)?;
    declare_sippers(&mut builder)?;
    builder.item(item::Definition::method(
        "Task::chain",
        receiver::Mode::Owned,
        task_chain_signature()?,
    )?)?;
    builder.item(item::Definition::method(
        "Task::map",
        receiver::Mode::Owned,
        task_map_signature()?,
    )?)?;
    builder.item(
        item::Definition::associated_function(
            "Subscription::none",
            subscription_none_signature()?,
        )?
        .discardable()
        .elidable(),
    )?;
    builder.item(item::Definition::method(
        "Subscription::map",
        receiver::Mode::Owned,
        subscription_map_signature()?,
    )?)?;
    builder.item(item::Definition::intrinsic_method(
        "Application::title",
        receiver::Mode::Owned,
        application_title_signature()?,
    )?)?;
    builder.item(item::Definition::intrinsic_method(
        "Application::theme",
        receiver::Mode::Owned,
        application_theme_signature()?,
    )?)?;
    builder.item(item::Definition::intrinsic_method(
        "Application::subscription",
        receiver::Mode::Owned,
        application_subscription_signature()?,
    )?)?;
    builder.item(item::Definition::intrinsic_method(
        "Application::centered",
        receiver::Mode::Owned,
        application_centered_signature()?,
    )?)?;
    builder.item(item::Definition::intrinsic_method(
        "Application::run",
        receiver::Mode::Owned,
        application_run_signature()?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::button",
        button_signature()?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::button",
        unary_widget_signature("widget::Button")?,
    )?)?;
    for name in BUILT_IN_TEXT_STYLES {
        builder.item(item::Definition::value(
            format!("widget::text::{name}"),
            type_ref::Type::registered("widget::text::Style")?,
        )?)?;
    }
    for name in BUILT_IN_CONTAINER_STYLES {
        builder.item(item::Definition::value(
            format!("widget::container::{name}"),
            type_ref::Type::registered("widget::container::Builtin")?,
        )?)?;
    }
    declare_styles(&mut builder)?;
    widget::style::declare(&mut builder)?;
    declare_palette(&mut builder)?;
    builder.item(item::Definition::intrinsic_macro(
        "widget::column",
        column_signature()?,
    )?)?;
    builder.item(item::Definition::intrinsic_macro(
        "widget::row",
        collection_signature("widget::Row")?,
    )?)?;
    // iced's `column(children)` and `row(children)` beside the macros of the
    // same name, with the `Column` and `Row` constructors they call.
    for (function, owner) in [
        ("widget::column", "widget::Column"),
        ("widget::row", "widget::Row"),
    ] {
        builder.item(item::Definition::function(
            function,
            children_signature(owner)?,
        )?)?;
        for name in ["with_children", "from_vec"] {
            builder.item(item::Definition::associated_function(
                format!("{owner}::{name}"),
                children_signature(owner)?,
            )?)?;
        }
        builder.item(item::Definition::associated_function(
            format!("{owner}::new"),
            generic_empty_signature(owner)?,
        )?)?;
    }
    // iced's `keyed_column(pairs)`: a Column whose children carry the key
    // of their `(key, element)` pair.
    builder.item(item::Definition::function(
        "widget::keyed_column",
        keyed_children_signature("widget::Column")?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::container",
        container_signature()?,
    )?)?;
    // iced's `center`, `center_x` and `center_y`: a container filling the
    // axis (or both) with its content centered there.
    for name in ["widget::center", "widget::center_x", "widget::center_y"] {
        builder.item(item::Definition::function(name, container_signature()?)?)?;
    }
    for input in [
        type_ref::Builtin::String,
        type_ref::Builtin::I64,
        type_ref::Builtin::Bool,
        type_ref::Builtin::Char,
        type_ref::Builtin::F64,
    ] {
        builder.item(item::Definition::function(
            "widget::text",
            text_signature(input)?,
        )?)?;
    }
    // `text!("{} items", n)`: `text(format!(..))`.
    builder.item(item::Definition::intrinsic_macro(
        "widget::text",
        text_signature(type_ref::Builtin::String)?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::text_input",
        text_input_signature()?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::checkbox",
        binary_control_signature("widget::Checkbox")?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::toggler",
        binary_control_signature("widget::Toggler")?,
    )?)?;
    for name in ["widget::slider", "widget::vertical_slider"] {
        for number in [type_ref::Builtin::I64, type_ref::Builtin::F64] {
            builder.item(item::Definition::function(name, slider_signature(number)?)?)?;
        }
    }
    declare_progress(&mut builder)?;
    declare_tooltip(&mut builder)?;
    // iced's logo: `iced(text_size)`.
    for pixels in PIXELS {
        builder.item(item::Definition::function(
            "widget::iced",
            fixed_builder_signature("widget::Logo", type_ref::Type::builtin(pixels))?,
        )?)?;
    }
    builder.item(item::Definition::function(
        "widget::pick_list",
        pick_list_signature()?,
    )?)?;
    pick_list_builders(&mut builder)?;
    declare_combo_box(&mut builder)?;
    builder.item(item::Definition::function(
        "widget::radio",
        radio_signature()?,
    )?)?;
    for name in ["size", "spacing", "text_size"] {
        pixels_builders(&mut builder, "widget::Radio", name)?;
    }
    length_builders(&mut builder, "widget::Radio", &["width"])?;
    builder.item(item::Definition::function(
        "widget::scrollable",
        unary_widget_signature("widget::Scrollable")?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::space",
        empty_widget_signature("widget::Space")?,
    )?)?;
    builder.module("widget::space")?;
    for name in ["horizontal", "vertical"] {
        builder.item(item::Definition::function(
            format!("widget::space::{name}"),
            empty_widget_signature("widget::Space")?,
        )?)?;
    }
    // iced's `void()`: what `None` is where a widget is expected; containers
    // drop it.
    builder.item(item::Definition::function(
        "widget::void",
        element_boxed_signature()?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "widget::component::Instance::new",
        component_instance_signature()?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::component",
        component_signature()?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::sensor",
        unary_widget_signature("widget::Sensor")?,
    )?)?;
    for name in ["on_show", "on_resize"] {
        builder.item(item::Definition::method(
            format!("widget::Sensor::{name}"),
            receiver::Mode::Owned,
            message_mapper_signature("widget::Sensor", type_ref::Type::registered("Size")?)?,
        )?)?;
    }
    builder.item(item::Definition::method(
        "widget::Sensor::on_hide",
        receiver::Mode::Owned,
        message_builder_signature("widget::Sensor", false)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::Button::on_press",
        receiver::Mode::Owned,
        message_builder_signature("widget::Button", false)?,
    )?)?;
    // iced's `_maybe` builders take Rust's `Option<Message>`.
    builder.item(item::Definition::method(
        "widget::Button::on_press_maybe",
        receiver::Mode::Owned,
        message_builder_signature("widget::Button", true)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::TextInput::on_submit",
        receiver::Mode::Owned,
        message_builder_signature("widget::TextInput", false)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::TextInput::on_submit_maybe",
        receiver::Mode::Owned,
        message_builder_signature("widget::TextInput", true)?,
    )?)?;
    padding_builders(&mut builder, "widget::Button")?;
    length_builders(&mut builder, "widget::Button", &["width", "height"])?;
    pixels_builders(&mut builder, "widget::Text", "size")?;
    length_builders(&mut builder, "widget::Text", &["width", "height"])?;
    for (name, input) in [
        ("color", type_ref::Type::registered("Color")?),
        ("font", type_ref::Type::registered("Font")?),
        ("line_height", type_ref::Type::builtin(type_ref::Builtin::F64)),
    ] {
        builder.item(item::Definition::method(
            format!("widget::Text::{name}"),
            receiver::Mode::Owned,
            builder_signature("widget::Text", input)?,
        )?)?;
    }
    builder.item(item::Definition::method(
        "widget::Text::align_x",
        receiver::Mode::Owned,
        fixed_builder_signature("widget::Text", type_ref::Type::registered("Alignment")?)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::Text::style",
        receiver::Mode::Owned,
        fixed_builder_signature(
            "widget::Text",
            type_ref::Type::registered("widget::text::Style")?,
        )?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::TextInput::on_input",
        receiver::Mode::Owned,
        message_mapper_builder_signature("widget::TextInput", type_ref::Builtin::String, false)?,
    )?)?;
    // iced's `_maybe` builders take an optional closure.
    builder.item(item::Definition::method(
        "widget::TextInput::on_input_maybe",
        receiver::Mode::Owned,
        message_mapper_builder_signature("widget::TextInput", type_ref::Builtin::String, true)?,
    )?)?;
    padding_builders(&mut builder, "widget::TextInput")?;
    pixels_builders(&mut builder, "widget::TextInput", "size")?;
    length_builders(&mut builder, "widget::TextInput", &["width"])?;
    builder.item(item::Definition::method(
        "widget::TextInput::secure",
        receiver::Mode::Owned,
        builtin_builder_signature("widget::TextInput", type_ref::Builtin::Bool)?,
    )?)?;
    for widget in ["widget::Checkbox", "widget::Toggler"] {
        builder.item(item::Definition::method(
            format!("{widget}::label"),
            receiver::Mode::Owned,
            builtin_builder_signature(widget, type_ref::Builtin::String)?,
        )?)?;
        builder.item(item::Definition::method(
            format!("{widget}::on_toggle"),
            receiver::Mode::Owned,
            message_mapper_builder_signature(widget, type_ref::Builtin::Bool, false)?,
        )?)?;
        builder.item(item::Definition::method(
            format!("{widget}::on_toggle_maybe"),
            receiver::Mode::Owned,
            message_mapper_builder_signature(widget, type_ref::Builtin::Bool, true)?,
        )?)?;
    }
    pixels_builders(&mut builder, "widget::Toggler", "size")?;
    pixels_builders(&mut builder, "widget::Toggler", "spacing")?;
    length_builders(&mut builder, "widget::Toggler", &["width"])?;
    pixels_builders(&mut builder, "widget::Checkbox", "size")?;
    pixels_builders(&mut builder, "widget::Checkbox", "spacing")?;
    length_builders(&mut builder, "widget::Checkbox", &["width"])?;
    // iced's slider takes `width(impl Into<Length>)` and `height(impl
    // Into<Pixels>)`, its vertical twin the reverse; one facade type has
    // both as lengths, pixels across the rail.
    for name in ["width", "height"] {
        let inputs = std::iter::once(type_ref::Type::registered("Length")?)
            .chain(PIXELS.map(type_ref::Type::builtin));
        for input in inputs {
            builder.item(item::Definition::method(
                format!("widget::Slider::{name}"),
                receiver::Mode::Owned,
                slider_input_signature(input)?,
            )?)?;
        }
    }
    for name in ["step", "default", "shift_step"] {
        builder.item(item::Definition::method(
            format!("widget::Slider::{name}"),
            receiver::Mode::Owned,
            slider_step_signature()?,
        )?)?;
    }
    pixels_builders(&mut builder, "widget::Column", "spacing")?;
    padding_builders(&mut builder, "widget::Column")?;
    builder.item(item::Definition::method(
        "widget::Column::align_x",
        receiver::Mode::Owned,
        column_align_x_signature()?,
    )?)?;
    // iced's `impl Into<alignment::Horizontal>`.
    builder.item(item::Definition::method(
        "widget::Column::align_x",
        receiver::Mode::Owned,
        registered_builder_signature("widget::Column", "alignment::Horizontal")?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::Container::align_x",
        receiver::Mode::Owned,
        registered_builder_signature("widget::Container", "alignment::Horizontal")?,
    )?)?;
    length_builders(&mut builder, "widget::Column", &["width", "height"])?;
    pixels_builders(&mut builder, "widget::Row", "spacing")?;
    padding_builders(&mut builder, "widget::Row")?;
    length_builders(&mut builder, "widget::Row", &["width", "height"])?;
    builder.item(item::Definition::method(
        "widget::Row::align_y",
        receiver::Mode::Owned,
        registered_builder_signature("widget::Row", "Alignment")?,
    )?)?;
    for owner in ["widget::Column", "widget::Row"] {
        // iced's `push(impl Into<Element>)`, so loops can build children.
        builders(
            &mut builder,
            owner,
            "push",
            [type_ref::Type::applied(
                "Element",
                [type_ref::Type::parameter("Message")?],
            )?],
        )?;
        // iced's `extend(impl IntoIterator<Item = impl Into<Element>>)`.
        builders(&mut builder, owner, "extend", [children()?])?;
    }
    padding_builders(&mut builder, "widget::Container")?;
    builders(
        &mut builder,
        "widget::Container",
        "style",
        [
            type_ref::Type::registered("widget::container::Builtin")?,
            // iced's `impl Fn(&Theme) -> container::Style`, as a named function.
            type_ref::Type::callable(
                type_ref::Type::registered("Theme")?,
                type_ref::Type::registered("widget::container::Style")?,
            ),
        ],
    )?;
    length_builders(
        &mut builder,
        "widget::Container",
        &["width", "height", "center_x", "center_y"],
    )?;
    for name in ["align_x", "align_y"] {
        builder.item(item::Definition::method(
            format!("widget::Container::{name}"),
            receiver::Mode::Owned,
            registered_builder_signature("widget::Container", "Alignment")?,
        )?)?;
    }
    length_builders(&mut builder, "widget::Space", &["width", "height"])?;
    length_builders(&mut builder, "widget::Scrollable", &["width", "height"])?;
    builder.item(item::Definition::method(
        "Element::map",
        receiver::Mode::Owned,
        element_map_signature()?,
    )?)?;
    // iced's `Widget::boxed`; every widget has it and `map` through `Widget`.
    builder.item(item::Definition::method(
        "Element::boxed",
        receiver::Mode::Owned,
        element_boxed_signature()?,
    )?)?;
    declare_canonical_values(&mut builder)?;
    declare_master_helpers(&mut builder)?;
    declare_native_tasks(&mut builder)?;
    declare_wrappers(&mut builder)?;
    declare_media(&mut builder)?;
    declare_rich_text(&mut builder)?;
    declare_canvas(&mut builder)?;
    widget::custom::declare::declare(&mut builder)?;
    declare_events(&mut builder)?;
    declare_event_variants(&mut builder)?;
    declare_font(&mut builder)?;
    declare_checkbox_icon(&mut builder)?;
    declare_angles(&mut builder)?;
    declare_scrollable(&mut builder)?;
    declare_table(&mut builder)?;
    // `font::Family::to_string` and `ContentFit::to_string`: their
    // `Display`, as a pick list's labels.
    for owner in ["font::Family", "ContentFit"] {
        let mut name = signature::Builder::new();
        name.output(type_ref::Type::builtin(type_ref::Builtin::String));
        builder.item(item::Definition::method(
            format!("{owner}::to_string"),
            receiver::Mode::Shared,
            name.finish(),
        )?)?;
    }

    builder.finish()
}

/// Build the validated compile-time `rawr_iced` extension contract.
///
/// Every iced type in this facade is retained through an exact export from the
/// authoritative [`facade`] definition.
pub fn extension(iced: &facade::Definition) -> Result<facade::Definition, host::Error> {
    extension_with_exports(Exports::new(iced)?, None)
}

/// The profile's extension carries the profile's exact seal, so application
/// semantics finalize its own types like the profile facade's.
pub(crate) fn extension_profile(
    iced: &facade::Definition,
    seal: [u8; 32],
) -> Result<facade::Definition, host::Error> {
    extension_with_exports(Exports::new(iced)?, Some(seal))
}

fn extension_with_exports(
    exports: Exports,
    seal: Option<[u8; 32]>,
) -> Result<facade::Definition, host::Error> {
    let mut builder = facade::Builder::new("rawr_iced", facade::Abi::new(4)?)?;
    if let Some(seal) = seal {
        builder.compatibility_seal(seal)?;
    }
    for module in [
        "task",
        "subscription",
        "theme",
        "time",
        "widget",
        "widget::children",
        "widget::draw",
        "widget::event",
        "widget::layout",
        "widget::mouse",
        "widget::operation",
        "widget::overlay",
    ] {
        builder.module(module)?;
    }

    builder.type_::<theme::Palette>("theme::Palette", &[], &[])?;
    builder.invocation_type::<renderer::Lease>("Renderer", &[])?;
    builder.type_::<widget::children::List>("widget::children::List", &["Message"], &[])?;
    builder.type_::<iced::Size<iced::Length>>(
        "widget::layout::Size",
        &[],
        OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<widget::layout::Input>("widget::layout::Input", &[], &[])?;
    builder.type_::<widget::layout::Node>("widget::layout::Node", &[], &[])?;
    builder.type_::<widget::event::Input>("widget::event::Input", &[], &[])?;
    builder.type_::<widget::event::Outcome>("widget::event::Outcome", &["Message"], &[])?;
    builder.type_::<widget::draw::Input>("widget::draw::Input", &[], &[])?;
    builder.type_::<widget::mouse::Input>("widget::mouse::Input", &[], &[])?;
    builder.type_::<widget::operation::Input>("widget::operation::Input", &[], &[])?;
    builder.type_::<widget::operation::Outcome>("widget::operation::Outcome", &[], &[])?;
    builder.type_::<widget::overlay::Input>("widget::overlay::Input", &[], &[])?;
    builder.type_::<widget::overlay::Value>("widget::overlay::Value", &["Message"], &[])?;

    builder.item(item::Definition::intrinsic_macro(
        "task::batch",
        external_batch_signature(&exports.task)?,
    )?)?;
    builder.item(item::Definition::intrinsic_macro(
        "subscription::batch",
        external_batch_signature(&exports.subscription)?,
    )?)?;
    builder.item(item::Definition::function(
        "task::script",
        script_signature(&exports.task)?,
    )?)?;
    // tokio, for a program without tokio (`resolve::std_reexport`).
    tokio_facade::declare(&mut builder, &exports.task, &exports.duration, &exports.instant)?;
    // std's `path` and `ffi` types (`resolve::std_reexport`).
    path_facade::declare(&mut builder)?;
    // std's `io` errors (`resolve::std_reexport`).
    io_facade::declare(&mut builder)?;
    builder.item(item::Definition::function(
        "theme::custom",
        extension_theme_signature(&exports.theme)?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "theme::Palette::new",
        string_constructor_signature("theme::Palette", 6)?,
    )?)?;
    for key in [type_ref::Builtin::I64, type_ref::Builtin::String] {
        builder.item(item::Definition::function(
            "widget::key",
            key_signature(&exports.element, key)?,
        )?)?;
    }
    builder.item(item::Definition::intrinsic_macro(
        "widget::children::list",
        extension_children_signature(&exports.element)?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "widget::layout::Size::new",
        extension_size_signature(&exports.length)?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "widget::layout::Size::fixed",
        integer_signature("widget::layout::Size", 2)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::layout::Input::resolve",
        receiver::Mode::Owned,
        layout_signature()?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::layout::Node::child",
        receiver::Mode::Owned,
        integer_signature("widget::layout::Node", 5)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::event::Input::is_press",
        receiver::Mode::Owned,
        bool_signature()?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::event::ignore",
        message_output_signature("widget::event::Outcome")?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::event::publish",
        message_transform_signature("widget::event::Outcome")?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::event::Outcome::when",
        receiver::Mode::Owned,
        event_outcome_when_signature()?,
    )?)?;
    builder.item(item::Definition::method(
        "Renderer::fill_quad",
        receiver::Mode::Mutable,
        extension_fill_signature(&exports, false)?,
    )?)?;
    builder.item(item::Definition::method(
        "Renderer::fill_rounded_quad",
        receiver::Mode::Mutable,
        extension_fill_signature(&exports, true)?,
    )?)?;
    for (name, output) in [
        ("bounds", &exports.rectangle),
        ("viewport", &exports.rectangle),
        ("text_color", &exports.color),
    ] {
        builder.item(item::Definition::method(
            format!("widget::draw::Input::{name}"),
            receiver::Mode::Owned,
            external_output_signature(output)?,
        )?)?;
    }
    builder.item(item::Definition::method(
        "widget::operation::Input::bounds",
        receiver::Mode::Owned,
        external_output_signature(&exports.rectangle)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::overlay::Input::viewport",
        receiver::Mode::Owned,
        external_output_signature(&exports.rectangle)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::overlay::Input::translation",
        receiver::Mode::Owned,
        external_output_signature(&exports.vector)?,
    )?)?;
    builder.item(item::Definition::method(
        "widget::mouse::Input::is_over",
        receiver::Mode::Owned,
        bool_signature()?,
    )?)?;
    for name in ["skip", "container", "traverse"] {
        builder.item(item::Definition::function(
            format!("widget::operation::{name}"),
            output_signature("widget::operation::Outcome")?,
        )?)?;
    }
    builder.item(item::Definition::function(
        "widget::overlay::none",
        message_output_signature("widget::overlay::Value")?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::overlay::some",
        extension_overlay_signature(&exports.element)?,
    )?)?;

    builder.finish()
}

/// Install the ordered compile-time facades into an engine without runtime modules.
pub fn install(engine: &mut Engine) -> Result<&mut Engine, host::Error> {
    let iced = facade()?;
    let rawr = extension(&iced)?;
    engine.register_host_facade(iced)?;
    engine.register_host_facade(rawr)
}

/// Build the exact ordered runtime facades used to bind an application launch.
///
/// The native descriptor layer binds Text, Button, Column, Row, Container,
/// their owned builder methods, and their authenticated projections to the
/// open widget boundary.
pub fn runtime_facades() -> Result<[host::runtime::Facade; 2], host::Error> {
    let iced = facade()?;
    let rawr = extension(&iced)?;
    let mut iced_builder = host::runtime::Builder::new(iced);
    let mut rawr_builder = host::runtime::Builder::new(rawr);
    subscription::bind(&mut iced_builder, &mut rawr_builder);
    task::bind(&mut iced_builder, &mut rawr_builder);
    tokio_facade::bind(&mut rawr_builder);
    path_facade::bind(&mut rawr_builder);
    io_facade::bind(&mut rawr_builder);
    theme::bind(&mut iced_builder, &mut rawr_builder);
    widget::bind(&mut iced_builder, &mut rawr_builder);
    Ok([iced_builder.finish(), rawr_builder.finish()])
}

/// Writes `relative` in the Cargo crate at `root`, marked `// rawr` unless
/// `source` already is, and the crate's `Cargo.toml` when it has none.
#[cfg(test)]
pub(crate) fn write_marked(root: &std::path::Path, relative: &str, source: impl AsRef<str>) {
    let manifest = root.join("Cargo.toml");
    if !manifest.exists() {
        std::fs::write(
            manifest,
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
    }
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = source.as_ref();
    if source.starts_with("// rawr\n") {
        std::fs::write(path, source).unwrap();
    } else {
        std::fs::write(path, format!("// rawr\n{source}")).unwrap();
    }
}

#[cfg(test)]
pub(crate) fn install_test_runtime(registry: &mut host::runtime::Registry) {
    for facade in runtime_facades().expect("the canonical runtime facades must build") {
        registry
            .install(facade)
            .expect("the canonical runtime facades must install in order");
    }
}

struct Exports {
    task: host::type_def::Export,
    duration: host::type_def::Export,
    instant: host::type_def::Export,
    subscription: host::type_def::Export,
    theme: host::type_def::Export,
    element: host::type_def::Export,
    color: host::type_def::Export,
    length: host::type_def::Export,
    rectangle: host::type_def::Export,
    vector: host::type_def::Export,
}

impl Exports {
    fn new(iced: &facade::Definition) -> Result<Self, host::Error> {
        Ok(Self {
            task: iced.export_type("Task")?,
            duration: iced.export_type("time::Duration")?,
            instant: iced.export_type("time::Instant")?,
            subscription: iced.export_type("Subscription")?,
            theme: iced.export_type("Theme")?,
            element: iced.export_type("Element")?,
            color: iced.export_type("Color")?,
            length: iced.export_type("Length")?,
            rectangle: iced.export_type("Rectangle")?,
            vector: iced.export_type("Vector")?,
        })
    }
}

fn external_type(
    export: &host::type_def::Export,
    arguments: impl IntoIterator<Item = type_ref::Type>,
) -> Result<type_ref::Type, host::Error> {
    type_ref::Type::external(export.clone(), arguments)
}

fn external_batch_signature(
    export: &host::type_def::Export,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    let plan = external_type(export, [type_ref::Type::parameter("Message")?])?;
    signature.parameter(plan.clone());
    signature.output(plan);
    Ok(signature.finish())
}

/// The `Task` an async body runs as: its step function, which takes `()`
/// or the frame it resumes from (`rawr::future`), and the body's output.
fn script_signature(
    export: &host::type_def::Export,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Resume", &[])?;
    signature.type_parameter("Output", &[])?;
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("Resume")?,
        type_ref::Type::parameter("Output")?,
    ));
    signature.output(external_type(export, [type_ref::Type::parameter("Output")?])?);
    Ok(signature.finish())
}

fn extension_theme_signature(
    theme: &host::type_def::Export,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    signature.parameter(type_ref::Type::registered("theme::Palette")?);
    signature.output(external_type(theme, [])?);
    Ok(signature.finish())
}

fn key_signature(
    element: &host::type_def::Export,
    key: type_ref::Builtin,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    let element = external_type(element, [type_ref::Type::parameter("Message")?])?;
    signature.parameter(element.clone());
    signature.parameter(type_ref::Type::builtin(key));
    signature.output(element);
    Ok(signature.finish())
}

fn extension_children_signature(
    element: &host::type_def::Export,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(external_type(
        element,
        [type_ref::Type::parameter("Message")?],
    )?);
    signature.output(type_ref::Type::applied(
        "widget::children::List",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn extension_size_signature(
    length: &host::type_def::Export,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    let length = external_type(length, [])?;
    signature.parameter(length.clone());
    signature.parameter(length);
    signature.output(type_ref::Type::registered("widget::layout::Size")?);
    Ok(signature.finish())
}

fn extension_fill_signature(
    exports: &Exports,
    rounded: bool,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.parameter(external_type(&exports.rectangle, [])?);
    signature.parameter(external_type(&exports.color, [])?);
    if rounded {
        signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    }
    Ok(signature.finish())
}

fn external_output_signature(
    output: &host::type_def::Export,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.output(external_type(output, [])?);
    Ok(signature.finish())
}

fn extension_overlay_signature(
    element: &host::type_def::Export,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(external_type(
        element,
        [type_ref::Type::parameter("Message")?],
    )?);
    signature.output(type_ref::Type::applied(
        "widget::overlay::Value",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn run_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    state_message_parameters(&mut signature)?;
    signature.type_parameter("Update", &[])?;
    signature.type_parameter("View", &[])?;
    signature.parameter(type_ref::Type::parameter("Update")?);
    signature.parameter(type_ref::Type::parameter("View")?);
    signature.output(type_ref::Type::registered("Result")?);
    Ok(signature.finish())
}

fn application_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    state_message_parameters(&mut signature)?;
    signature.type_parameter("Boot", &[])?;
    signature.type_parameter("Update", &[])?;
    signature.type_parameter("View", &[])?;
    signature.parameter(type_ref::Type::parameter("Boot")?);
    signature.parameter(type_ref::Type::parameter("Update")?);
    signature.parameter(type_ref::Type::parameter("View")?);
    signature.output(type_ref::Type::applied(
        "Application",
        [
            type_ref::Type::parameter("State")?,
            type_ref::Type::parameter("Message")?,
        ],
    )?);
    Ok(signature.finish())
}

fn application_run_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.output(type_ref::Type::registered("Result")?);
    Ok(signature.finish())
}

fn application_centered_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    state_message_parameters(&mut signature)?;
    signature.output(type_ref::Type::applied(
        "Application",
        [
            type_ref::Type::parameter("State")?,
            type_ref::Type::parameter("Message")?,
        ],
    )?);
    Ok(signature.finish())
}

fn application_title_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    state_message_parameters(&mut signature)?;
    signature.type_parameter("TitleFn", &[])?;
    signature.parameter(type_ref::Type::parameter("TitleFn")?);
    signature.output(type_ref::Type::applied(
        "Application",
        [
            type_ref::Type::parameter("State")?,
            type_ref::Type::parameter("Message")?,
        ],
    )?);
    Ok(signature.finish())
}

fn application_theme_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    state_message_parameters(&mut signature)?;
    signature.type_parameter("ThemeFn", &[])?;
    signature.parameter(type_ref::Type::parameter("ThemeFn")?);
    signature.output(type_ref::Type::applied(
        "Application",
        [
            type_ref::Type::parameter("State")?,
            type_ref::Type::parameter("Message")?,
        ],
    )?);
    Ok(signature.finish())
}

fn application_subscription_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    state_message_parameters(&mut signature)?;
    signature.type_parameter("SubscriptionFn", &[])?;
    signature.parameter(type_ref::Type::parameter("SubscriptionFn")?);
    signature.output(type_ref::Type::applied(
        "Application",
        [
            type_ref::Type::parameter("State")?,
            type_ref::Type::parameter("Message")?,
        ],
    )?);
    Ok(signature.finish())
}

fn task_none_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn task_done_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::parameter("Message")?);
    signature.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn task_batch_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    let plan = type_ref::Type::applied("Task", [type_ref::Type::parameter("Message")?])?;
    signature.parameter(plan.clone());
    signature.output(plan);
    Ok(signature.finish())
}

fn task_perform_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Output", &[])?;
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::parameter("Output")?);
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("Output")?,
        type_ref::Type::parameter("Message")?,
    ));
    signature.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

/// iced's `Task::sip(sipper, on_progress, on_output)`, `Task::abortable()`
/// with its `task::Handle`, and `Subscription::run(builder)` over a Rust
/// function that returns a sipper.
fn declare_sippers(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let parameter = |name: &str| type_ref::Type::parameter(name);
    let task = |message: &str| type_ref::Type::applied("Task", [parameter(message)?]);
    let sipper = || type_ref::Type::applied("task::Sipper", [parameter("Output")?, parameter("Progress")?]);
    let handle = || type_ref::Type::registered("task::Handle");

    // `futures::future::join(a, b)` and `try_join(a, b)`, which iced
    // re-exports: both futures run at once.
    let mut join = signature::Builder::new();
    join.type_parameter("A", &[])?;
    join.type_parameter("B", &[])?;
    join.parameter(task("A")?);
    join.parameter(task("B")?);
    join.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::tuple([parameter("A")?, parameter("B")?])?],
    )?);
    builder.item(item::Definition::function("futures::future::join", join.finish())?)?;
    let fallible = |name: &str| -> Result<type_ref::Type, host::Error> {
        type_ref::Type::applied(
            "Task",
            [type_ref::Type::result(parameter(name)?, parameter("E")?)],
        )
    };
    let mut try_join = signature::Builder::new();
    try_join.type_parameter("A", &[])?;
    try_join.type_parameter("B", &[])?;
    try_join.type_parameter("E", &[])?;
    try_join.parameter(fallible("A")?);
    try_join.parameter(fallible("B")?);
    try_join.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::result(
            type_ref::Type::tuple([parameter("A")?, parameter("B")?])?,
            parameter("E")?,
        )],
    )?);
    builder.item(item::Definition::function(
        "futures::future::try_join",
        try_join.finish(),
    )?)?;

    // `sipper(async |mut progress| { progress.send(value).await; .. })`.
    let sender = || type_ref::Type::applied("task::Sender", [parameter("Progress")?]);
    let mut script = signature::Builder::new();
    script.type_parameter("Output", &[])?;
    script.type_parameter("Progress", &[])?;
    script.parameter(type_ref::Type::callable(sender()?, task("Output")?));
    script.output(sipper()?);
    builder.item(item::Definition::function("task::sipper", script.finish())?)?;
    let mut send = signature::Builder::new();
    send.type_parameter("Progress", &[])?;
    send.parameter(parameter("Progress")?);
    send.output(type_ref::Type::applied("Task", [type_ref::Type::unit()])?);
    builder.item(item::Definition::method(
        "task::Sender::send",
        receiver::Mode::Shared,
        send.finish(),
    )?)?;

    let mut sip = signature::Builder::new();
    sip.type_parameter("Output", &[])?;
    sip.type_parameter("Progress", &[])?;
    sip.type_parameter("Message", &[])?;
    sip.parameter(sipper()?);
    sip.parameter(type_ref::Type::callable(parameter("Progress")?, parameter("Message")?));
    sip.parameter(type_ref::Type::callable(parameter("Output")?, parameter("Message")?));
    sip.output(task("Message")?);
    builder.item(item::Definition::associated_function("Task::sip", sip.finish())?)?;

    let mut abortable = signature::Builder::new();
    abortable.type_parameter("Message", &[])?;
    abortable.output(type_ref::Type::tuple([task("Message")?, handle()?])?);
    builder.item(item::Definition::method(
        "Task::abortable",
        receiver::Mode::Owned,
        abortable.finish(),
    )?)?;

    let mut abort_on_drop = signature::Builder::new();
    abort_on_drop.output(handle()?);
    builder.item(item::Definition::method(
        "task::Handle::abort_on_drop",
        receiver::Mode::Owned,
        abort_on_drop.finish(),
    )?)?;
    let mut abort = signature::Builder::new();
    abort.output(type_ref::Type::unit());
    builder.item(item::Definition::method(
        "task::Handle::abort",
        receiver::Mode::Shared,
        abort.finish(),
    )?)?;
    let mut is_aborted = signature::Builder::new();
    is_aborted.output(type_ref::Type::builtin(type_ref::Builtin::Bool));
    builder.item(item::Definition::method(
        "task::Handle::is_aborted",
        receiver::Mode::Shared,
        is_aborted.finish(),
    )?)?;

    // `Subscription::run(builder)`: the compiler passes the sipper the
    // Rust function returns and the function's path, which is the
    // subscription's identity, as iced's is the function pointer.
    // The source name the compiler routes to the stream operation.
    let mut source = signature::Builder::new();
    source.type_parameter("Output", &[])?;
    source.type_parameter("Progress", &[])?;
    source.parameter(sipper()?);
    source.output(type_ref::Type::applied("Subscription", [parameter("Progress")?])?);
    builder.item(item::Definition::associated_function("Subscription::run", source.finish())?)?;
    let mut run = signature::Builder::new();
    run.type_parameter("Output", &[])?;
    run.type_parameter("Progress", &[])?;
    run.parameter(sipper()?);
    run.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    run.output(parameter("Progress")?);
    builder.item(item::Definition::stream_operation(
        "subscription::run",
        run.finish(),
        host::stream::Version::new(1),
    )?)?;
    Ok(())
}

/// `Task::future(future)`. A Rust future crosses into Rawr as a `Task` of
/// its output, so this takes that task.
fn task_future_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Output", &[])?;
    let task = type_ref::Type::applied("Task", [type_ref::Type::parameter("Output")?])?;
    signature.parameter(task.clone());
    signature.output(task);
    Ok(signature.finish())
}

fn task_discard_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.type_parameter("Other", &[])?;
    signature.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::parameter("Other")?],
    )?);
    Ok(signature.finish())
}

fn task_chain_signature() -> Result<signature::Definition, host::Error> {
    task_batch_signature()
}

fn task_map_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.type_parameter("Parent", &[])?;
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("Message")?,
        type_ref::Type::parameter("Parent")?,
    ));
    signature.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::parameter("Parent")?],
    )?);
    Ok(signature.finish())
}

fn subscription_none_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.output(type_ref::Type::applied(
        "Subscription",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn subscription_map_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.type_parameter("Parent", &[])?;
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("Message")?,
        type_ref::Type::parameter("Parent")?,
    ));
    signature.output(type_ref::Type::applied(
        "Subscription",
        [type_ref::Type::parameter("Parent")?],
    )?);
    Ok(signature.finish())
}

fn button_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    signature.output(type_ref::Type::applied(
        "widget::Button",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn column_signature() -> Result<signature::Definition, host::Error> {
    collection_signature("widget::Column")
}

fn collection_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::applied(
        "Element",
        [type_ref::Type::parameter("Message")?],
    )?);
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

/// `impl IntoIterator<Item = impl Into<Element>>`: a `Vec` or array of
/// widgets, each converted into an Element.
fn children() -> Result<type_ref::Type, host::Error> {
    Ok(type_ref::Type::slice(type_ref::Type::applied(
        "Element",
        [type_ref::Type::parameter("Message")?],
    )?))
}

fn children_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(children()?);
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn keyed_children_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Key", &[])?;
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::slice(type_ref::Type::tuple([
        type_ref::Type::parameter("Key")?,
        type_ref::Type::applied("Element", [type_ref::Type::parameter("Message")?])?,
    ])?));
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn generic_empty_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn container_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::applied(
        "Element",
        [type_ref::Type::parameter("Message")?],
    )?);
    signature.output(type_ref::Type::applied(
        "widget::Container",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn unary_widget_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::applied(
        "Element",
        [type_ref::Type::parameter("Message")?],
    )?);
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn empty_widget_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.output(type_ref::Type::registered(output)?);
    Ok(signature.finish())
}

fn text_signature(input: type_ref::Builtin) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.parameter(type_ref::Type::builtin(input));
    signature.output(type_ref::Type::registered("widget::Text")?);
    Ok(signature.finish())
}

fn text_input_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    signature.output(type_ref::Type::applied(
        "widget::TextInput",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn binary_control_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::Bool));
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

/// `slider(range, value, on_change)` over `i64` or `f64`.
fn slider_signature(number: type_ref::Builtin) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::builtin(if number == type_ref::Builtin::F64 {
        type_ref::Builtin::FloatInclusiveRange
    } else {
        type_ref::Builtin::InclusiveRange
    }));
    signature.parameter(type_ref::Type::builtin(number));
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::builtin(number),
        type_ref::Type::parameter("Message")?,
    ));
    signature.output(type_ref::Type::applied(
        "widget::Slider",
        [type_ref::Type::builtin(number), type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

/// iced's `progress_bar(range, value)`, and `transition(value, init, view)`
/// with the `Animation` it takes. A Rawr view is drawn once per state, so a
/// transition shows its value settled: `interpolate` is that value.
fn declare_progress(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let mut progress = signature::Builder::new();
    progress.parameter(type_ref::Type::builtin(type_ref::Builtin::FloatInclusiveRange));
    progress.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
    progress.output(type_ref::Type::registered("widget::ProgressBar")?);
    builder.item(item::Definition::function("widget::progress_bar", progress.finish())?)?;
    builder.item(item::Definition::method(
        "widget::ProgressBar::vertical",
        receiver::Mode::Owned,
        fixed_builder_signature(
            "widget::ProgressBar",
            type_ref::Type::builtin(type_ref::Builtin::Bool),
        )?,
    )?)?;
    length_builders(builder, "widget::ProgressBar", &["length", "girth"])?;

    declare_animation(builder)?;
    for name in EASINGS {
        builder.item(item::Definition::associated_value(
            format!("animation::Easing::{name}"),
            type_ref::Type::registered("animation::Easing")?,
        )?)?;
    }

    let mut transition = signature::Builder::new();
    transition.type_parameter("Message", &[])?;
    transition.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
    // `impl Fn() -> Animation`: a callable of no inputs takes `()`.
    transition.parameter(type_ref::Type::callable(
        type_ref::Type::unit(),
        float_animation()?,
    ));
    transition.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([
            float_animation()?,
            type_ref::Type::registered("time::Instant")?,
        ])?,
        type_ref::Type::applied("Element", [type_ref::Type::parameter("Message")?])?,
    ));
    transition.output(type_ref::Type::applied(
        "widget::Transition",
        [type_ref::Type::parameter("Message")?],
    )?);
    builder.item(item::Definition::function("widget::transition", transition.finish())?)?;
    Ok(())
}

/// iced's `Animation<f32>`, as `transition` takes it.
fn float_animation() -> Result<type_ref::Type, host::Error> {
    type_ref::Type::applied("Animation", [type_ref::Type::builtin(type_ref::Builtin::F64)])
}

/// iced's `Animation<T>` of a `bool` or an `f32`: `new`, its builders, `go`
/// and `go_mut` at an `Instant`, `is_animating`, `value`, `remaining` and
/// `interpolate`. A `bool` animation interpolates between two numbers; a
/// number animation interpolates its own value.
fn declare_animation(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let t = || type_ref::Type::parameter("T");
    let animation = || type_ref::Type::applied("Animation", [t()?]);
    let instant = || type_ref::Type::registered("time::Instant");
    let duration = || type_ref::Type::registered("time::Duration");
    let f64 = || type_ref::Type::builtin(type_ref::Builtin::F64);
    let method = |builder: &mut facade::Builder,
                  name: &str,
                  mode: receiver::Mode,
                  inputs: Vec<type_ref::Type>,
                  output: type_ref::Type|
     -> Result<(), host::Error> {
        let mut signature = signature::Builder::new();
        signature.type_parameter("T", &[])?;
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        builder.item(item::Definition::method(
            format!("Animation::{name}"),
            mode,
            signature.finish(),
        )?)?;
        Ok(())
    };
    let mut new = signature::Builder::new();
    new.type_parameter("T", &[])?;
    new.parameter(t()?).output(animation()?);
    builder.item(item::Definition::associated_function("Animation::new", new.finish())?)?;
    for name in [
        "very_quick",
        "quick",
        "slow",
        "very_slow",
        "repeat_forever",
        "auto_reverse",
    ] {
        method(builder, name, receiver::Mode::Owned, Vec::new(), animation()?)?;
    }
    method(
        builder,
        "easing",
        receiver::Mode::Owned,
        vec![type_ref::Type::registered("animation::Easing")?],
        animation()?,
    )?;
    for name in ["duration", "delay"] {
        method(builder, name, receiver::Mode::Owned, vec![duration()?], animation()?)?;
    }
    method(
        builder,
        "repeat",
        receiver::Mode::Owned,
        vec![type_ref::Type::builtin(type_ref::Builtin::I64)],
        animation()?,
    )?;
    method(builder, "go", receiver::Mode::Owned, vec![t()?, instant()?], animation()?)?;
    method(
        builder,
        "go_mut",
        receiver::Mode::Mutable,
        vec![t()?, instant()?],
        type_ref::Type::unit(),
    )?;
    method(
        builder,
        "is_animating",
        receiver::Mode::Shared,
        vec![instant()?],
        type_ref::Type::builtin(type_ref::Builtin::Bool),
    )?;
    method(builder, "value", receiver::Mode::Shared, Vec::new(), t()?)?;
    method(builder, "remaining", receiver::Mode::Shared, vec![instant()?], duration()?)?;
    method(builder, "interpolate", receiver::Mode::Shared, vec![instant()?], f64())?;
    method(
        builder,
        "interpolate",
        receiver::Mode::Shared,
        vec![f64(), f64(), instant()?],
        f64(),
    )?;
    Ok(())
}

/// iced's `tooltip(content, tooltip, position)` and its `Position`, a facade
/// enum of unit variants that `match` compares by `==`;
/// `alignment::Horizontal`; and `time::seconds`/`milliseconds` for `delay`.
fn declare_tooltip(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for name in TOOLTIP_POSITIONS {
        builder.item(item::Definition::associated_value(
            format!("widget::tooltip::Position::{name}"),
            type_ref::Type::registered("widget::tooltip::Position")?,
        )?)?;
    }
    for name in ["Left", "Center", "Right"] {
        builder.item(item::Definition::associated_value(
            format!("alignment::Horizontal::{name}"),
            type_ref::Type::registered("alignment::Horizontal")?,
        )?)?;
    }
    for name in ["seconds", "milliseconds"] {
        builder.item(item::Definition::function(
            format!("time::{name}"),
            integer_signature("time::Duration", 1)?,
        )?)?;
    }
    for tooltip in [
        type_ref::Type::applied("Element", [type_ref::Type::parameter("Message")?])?,
        type_ref::Type::builtin(type_ref::Builtin::String),
    ] {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Message", &[])?;
        signature.parameter(type_ref::Type::applied(
            "Element",
            [type_ref::Type::parameter("Message")?],
        )?);
        signature.parameter(tooltip);
        signature.parameter(type_ref::Type::registered("widget::tooltip::Position")?);
        signature.output(type_ref::Type::applied(
            "widget::Tooltip",
            [type_ref::Type::parameter("Message")?],
        )?);
        builder.item(item::Definition::function("widget::tooltip", signature.finish())?)?;
    }
    pixels_builders(builder, "widget::Tooltip", "gap")?;
    builders(
        builder,
        "widget::Tooltip",
        "delay",
        [type_ref::Type::registered("time::Duration")?],
    )?;
    builders(
        builder,
        "widget::Tooltip",
        "snap_within_viewport",
        [type_ref::Type::builtin(type_ref::Builtin::Bool)],
    )?;
    builders(
        builder,
        "widget::Tooltip",
        "style",
        [
            type_ref::Type::registered("widget::container::Builtin")?,
            type_ref::Type::callable(
                type_ref::Type::registered("Theme")?,
                type_ref::Type::registered("widget::container::Style")?,
            ),
        ],
    )?;
    Ok(())
}

/// iced master's shorthands and small helpers: `Left`, `Right`, `Top` and
/// `Bottom` beside `Center`; `left`, `right`, `top` and `bottom` beside
/// `center`; `rule::horizontal`/`vertical`; `stack`; and `time::hours`/
/// `minutes`.
fn declare_master_helpers(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for name in ["Top", "Center", "Bottom"] {
        builder.item(item::Definition::associated_value(
            format!("alignment::Vertical::{name}"),
            type_ref::Type::registered("alignment::Vertical")?,
        )?)?;
    }
    for (name, owner) in [
        ("Left", "alignment::Horizontal"),
        ("Right", "alignment::Horizontal"),
        ("Top", "alignment::Vertical"),
        ("Bottom", "alignment::Vertical"),
    ] {
        builder.item(item::Definition::value(name, type_ref::Type::registered(owner)?)?)?;
    }
    // iced's `impl Into<alignment::Horizontal>` and `Into<Vertical>`.
    builder.item(item::Definition::method(
        "widget::Text::align_x",
        receiver::Mode::Owned,
        fixed_builder_signature("widget::Text", type_ref::Type::registered("alignment::Horizontal")?)?,
    )?)?;
    for input in ["alignment::Vertical", "Alignment"] {
        builder.item(item::Definition::method(
            "widget::Text::align_y",
            receiver::Mode::Owned,
            fixed_builder_signature("widget::Text", type_ref::Type::registered(input)?)?,
        )?)?;
    }
    // iced's `Text::center`: centered on both axes.
    builder.item(item::Definition::method(
        "widget::Text::center",
        receiver::Mode::Owned,
        output_signature("widget::Text")?,
    )?)?;
    // iced's `Text::color_maybe` takes `Option<impl Into<Color>>`.
    builder.item(item::Definition::method(
        "widget::Text::color_maybe",
        receiver::Mode::Owned,
        fixed_builder_signature(
            "widget::Text",
            type_ref::Type::option(type_ref::Type::registered("Color")?),
        )?,
    )?)?;
    for owner in ["widget::Row", "widget::Container"] {
        builder.item(item::Definition::method(
            format!("{owner}::align_y"),
            receiver::Mode::Owned,
            registered_builder_signature(owner, "alignment::Vertical")?,
        )?)?;
    }
    length_builders(
        builder,
        "widget::Container",
        &["center", "align_left", "align_right", "align_top", "align_bottom"],
    )?;
    for name in ["widget::left", "widget::right", "widget::top", "widget::bottom"] {
        builder.item(item::Definition::function(name, container_signature()?)?)?;
    }
    for name in ["widget::rule::horizontal", "widget::rule::vertical"] {
        for pixels in PIXELS {
            let mut signature = signature::Builder::new();
            signature.type_parameter("Message", &[])?;
            signature.parameter(type_ref::Type::builtin(pixels));
            signature.output(type_ref::Type::applied(
                "Element",
                [type_ref::Type::parameter("Message")?],
            )?);
            builder.item(item::Definition::function(name, signature.finish())?)?;
        }
    }
    builder.item(item::Definition::intrinsic_macro(
        "widget::stack",
        collection_signature("widget::Stack")?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::stack",
        children_signature("widget::Stack")?,
    )?)?;
    for name in ["with_children", "from_vec"] {
        builder.item(item::Definition::associated_function(
            format!("widget::Stack::{name}"),
            children_signature("widget::Stack")?,
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "widget::Stack::new",
        generic_empty_signature("widget::Stack")?,
    )?)?;
    length_builders(builder, "widget::Stack", &["width", "height"])?;
    builders(
        builder,
        "widget::Stack",
        "push",
        [type_ref::Type::applied(
            "Element",
            [type_ref::Type::parameter("Message")?],
        )?],
    )?;
    builders(builder, "widget::Stack", "extend", [children()?])?;
    for name in ["hours", "minutes"] {
        builder.item(item::Definition::function(
            format!("time::{name}"),
            integer_signature("time::Duration", 1)?,
        )?)?;
    }
    // `Duration::from_millis(10)` and kin, and its readers.
    for name in ["from_secs", "from_millis", "from_micros", "from_nanos"] {
        builder.item(item::Definition::associated_function(
            format!("time::Duration::{name}"),
            integer_signature("time::Duration", 1)?,
        )?)?;
    }
    for name in ["from_secs_f64", "from_secs_f32"] {
        builder.item(item::Definition::associated_function(
            format!("time::Duration::{name}"),
            repeated_builtin_signature(type_ref::Builtin::F64, 1, "time::Duration")?,
        )?)?;
    }
    for (name, output) in [
        ("as_secs", type_ref::Builtin::U64),
        ("as_millis", type_ref::Builtin::I64),
        ("as_micros", type_ref::Builtin::I64),
        ("as_nanos", type_ref::Builtin::I64),
        ("as_secs_f64", type_ref::Builtin::F64),
        ("as_secs_f32", type_ref::Builtin::F64),
    ] {
        let mut reader = signature::Builder::new();
        reader.output(type_ref::Type::builtin(output));
        builder.item(item::Definition::method(
            format!("time::Duration::{name}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
    }
    // `a + b`, `a - b` of durations and `d * n`, `d / n`: their operators,
    // the methods named for `std::ops`' traits.
    let duration = || type_ref::Type::registered("time::Duration");
    let instant = || type_ref::Type::registered("time::Instant");
    for (owner, name, input, output) in [
        ("time::Duration", "add", duration()?, duration()?),
        ("time::Duration", "sub", duration()?, duration()?),
        ("time::Duration", "mul", type_ref::Type::builtin(type_ref::Builtin::I64), duration()?),
        ("time::Duration", "div", type_ref::Type::builtin(type_ref::Builtin::I64), duration()?),
        ("time::Instant", "sub", instant()?, duration()?),
        ("time::Instant", "sub", duration()?, instant()?),
        ("time::Instant", "add", duration()?, instant()?),
    ] {
        let mut operator = signature::Builder::new();
        operator.parameter(input);
        operator.output(output);
        builder.item(item::Definition::method(
            format!("{owner}::{name}"),
            receiver::Mode::Owned,
            operator.finish(),
        )?)?;
    }
    for (name, output) in [
        ("subsec_millis", type_ref::Type::builtin(type_ref::Builtin::U32)),
        ("subsec_micros", type_ref::Type::builtin(type_ref::Builtin::U32)),
        ("subsec_nanos", type_ref::Type::builtin(type_ref::Builtin::U32)),
        ("is_zero", type_ref::Type::builtin(type_ref::Builtin::Bool)),
    ] {
        let mut reader = signature::Builder::new();
        reader.output(output);
        builder.item(item::Definition::method(
            format!("time::Duration::{name}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
    }
    builder.item(item::Definition::associated_value("time::Duration::ZERO", duration()?)?)?;
    builder.item(item::Definition::associated_function(
        "time::Instant::now",
        output_signature("time::Instant")?,
    )?)?;
    builder.item(item::Definition::method(
        "time::Instant::elapsed",
        receiver::Mode::Shared,
        output_signature("time::Duration")?,
    )?)?;
    let mut since = signature::Builder::new();
    since.parameter(instant()?);
    since.output(duration()?);
    builder.item(item::Definition::method(
        "time::Instant::duration_since",
        receiver::Mode::Shared,
        since.finish(),
    )?)?;
    // `size.width`, `point.x` and kin, read as fields.
    for (owner, fields) in [
        ("Size", &["width", "height"][..]),
        ("Point", &["x", "y"][..]),
        ("Vector", &["x", "y"][..]),
        ("Rectangle", &["x", "y", "width", "height"][..]),
        ("Color", &["r", "g", "b", "a"][..]),
    ] {
        for field in fields {
            let mut reader = signature::Builder::new();
            reader.output(type_ref::Type::builtin(type_ref::Builtin::F64));
            builder.item(item::Definition::method(
                format!("{owner}::{field}"),
                receiver::Mode::Shared,
                reader.finish(),
            )?)?;
        }
    }
    // `offset.x = 2.0` and `Size { width: 1.0, height: 2.0 }`: a field
    // set through its owned builder.
    for (owner, fields) in [
        ("Size", &["width", "height"][..]),
        ("Point", &["x", "y"][..]),
        ("Vector", &["x", "y"][..]),
    ] {
        for field in fields {
            for input in PIXELS {
                let mut setter = signature::Builder::new();
                setter.parameter(type_ref::Type::builtin(input));
                setter.output(type_ref::Type::registered(owner)?);
                builder.item(item::Definition::method(
                    format!("{owner}::{field}"),
                    receiver::Mode::Owned,
                    setter.finish(),
                )?)?;
            }
        }
    }
    builder.item(item::Definition::intrinsic_macro(
        "color",
        repeated_builtin_signature(type_ref::Builtin::I64, 1, "Color")?,
    )?)?;
    // A literal's channels: `Color { a: 0.8, ..Color::BLACK }`.
    for name in ["r", "g", "b", "a"] {
        let mut setter = signature::Builder::new();
        setter.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
        setter.output(type_ref::Type::registered("Color")?);
        builder.item(item::Definition::method(
            format!("Color::{name}"),
            receiver::Mode::Owned,
            setter.finish(),
        )?)?;
    }
    // `color.scale_alpha(0.5)` and `color.inverse()`.
    let mut scale_alpha = signature::Builder::new();
    scale_alpha.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
    scale_alpha.output(type_ref::Type::registered("Color")?);
    builder.item(item::Definition::method(
        "Color::scale_alpha",
        receiver::Mode::Owned,
        scale_alpha.finish(),
    )?)?;
    builder.item(item::Definition::method(
        "Color::inverse",
        receiver::Mode::Owned,
        output_signature("Color")?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "time::Duration::default",
        output_signature("time::Duration")?,
    )?)?;
    Ok(())
}

/// iced's `lazy`, `hover`, `opaque`, `responsive`, `pin` and `mouse_area`.
/// `lazy` calls its view each time the view is drawn: a Rawr view is
/// recomputed as a whole, so there is no older content to keep.
fn declare_wrappers(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let element = |message: &str| -> Result<type_ref::Type, host::Error> {
        type_ref::Type::applied("Element", [type_ref::Type::parameter(message)?])
    };
    let mut lazy = signature::Builder::new();
    lazy.type_parameter("D", &[])?;
    lazy.type_parameter("Message", &[])?;
    lazy.parameter(type_ref::Type::parameter("D")?);
    lazy.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("D")?,
        element("Message")?,
    ));
    lazy.output(element("Message")?);
    builder.item(item::Definition::function("widget::lazy", lazy.finish())?)?;
    let mut hover = signature::Builder::new();
    hover.type_parameter("Message", &[])?;
    hover.parameter(element("Message")?);
    hover.parameter(element("Message")?);
    hover.output(element("Message")?);
    builder.item(item::Definition::function("widget::hover", hover.finish())?)?;
    let mut opaque = signature::Builder::new();
    opaque.type_parameter("Message", &[])?;
    opaque.parameter(element("Message")?);
    opaque.output(element("Message")?);
    builder.item(item::Definition::function("widget::opaque", opaque.finish())?)?;
    let mut responsive = signature::Builder::new();
    responsive.type_parameter("Message", &[])?;
    responsive.parameter(type_ref::Type::callable(
        type_ref::Type::registered("Size")?,
        element("Message")?,
    ));
    responsive.output(element("Message")?);
    builder.item(item::Definition::function("widget::responsive", responsive.finish())?)?;

    builder.item(item::Definition::function(
        "widget::pin",
        unary_widget_signature("widget::Pin")?,
    )?)?;
    builder.item(item::Definition::function(
        "widget::float",
        unary_widget_signature("widget::Float")?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "widget::Float::new",
        unary_widget_signature("widget::Float")?,
    )?)?;
    builders(
        builder,
        "widget::Float",
        "scale",
        [type_ref::Type::builtin(type_ref::Builtin::F64)],
    )?;
    builders(
        builder,
        "widget::Float",
        "translate",
        [type_ref::Type::callable(
            type_ref::Type::tuple([
                type_ref::Type::registered("Rectangle")?,
                type_ref::Type::registered("Rectangle")?,
            ])?,
            type_ref::Type::registered("Vector")?,
        )],
    )?;
    pixels_builders(builder, "widget::Pin", "x")?;
    pixels_builders(builder, "widget::Pin", "y")?;
    builders(builder, "widget::Pin", "position", [type_ref::Type::registered("Point")?])?;
    length_builders(builder, "widget::Pin", &["width", "height"])?;

    builder.item(item::Definition::function(
        "widget::mouse_area",
        unary_widget_signature("widget::MouseArea")?,
    )?)?;
    for name in MOUSE_AREA_MESSAGES {
        builder.item(item::Definition::method(
            format!("widget::MouseArea::{name}"),
            receiver::Mode::Owned,
            message_builder_signature("widget::MouseArea", false)?,
        )?)?;
    }
    builder.item(item::Definition::method(
        "widget::MouseArea::on_move",
        receiver::Mode::Owned,
        message_mapper_signature("widget::MouseArea", type_ref::Type::registered("Point")?)?,
    )?)?;
    builders(
        builder,
        "widget::MouseArea",
        "interaction",
        [type_ref::Type::registered("mouse::Interaction")?],
    )?;
    Ok(())
}

/// iced's `image`, `svg` and `qr_code` widgets, their handles and data,
/// and `ContentFit`.
fn declare_media(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let string = || type_ref::Type::builtin(type_ref::Builtin::String);
    for (module, handle, function, widget) in [
        ("widget::image", "widget::image::Handle", "widget::image", "widget::Image"),
        ("widget::svg", "widget::svg::Handle", "widget::svg", "widget::Svg"),
    ] {
        // `from_path`, and `impl Into<Bytes>` for the rest: a `Vec<u8>`, a
        // `&[u8]` or a byte string.
        let bytes = || type_ref::Type::slice(type_ref::Type::builtin(type_ref::Builtin::U8));
        let u32_ = || type_ref::Type::builtin(type_ref::Builtin::U32);
        let constructors = if handle == "widget::image::Handle" {
            vec![
                ("from_path", vec![string()]),
                ("from_bytes", vec![bytes()]),
                ("from_rgba", vec![u32_(), u32_(), bytes()]),
            ]
        } else {
            vec![("from_path", vec![string()]), ("from_memory", vec![bytes()])]
        };
        for (name, parameters) in constructors {
            let mut signature = signature::Builder::new();
            for parameter in parameters {
                signature.parameter(parameter);
            }
            signature.output(type_ref::Type::registered(handle)?);
            builder.item(item::Definition::associated_function(
                format!("{handle}::{name}"),
                signature.finish(),
            )?)?;
        }
        // `impl Into<Handle>`: a handle, or a path.
        for input in [type_ref::Type::registered(handle)?, string()] {
            let mut signature = signature::Builder::new();
            signature.parameter(input);
            signature.output(type_ref::Type::registered(widget)?);
            builder.item(item::Definition::function(function, signature.finish())?)?;
        }
        let _ = module;
        length_builders(builder, widget, &["width", "height"])?;
        builders(builder, widget, "content_fit", [type_ref::Type::registered("ContentFit")?])?;
        builders(builder, widget, "opacity", [type_ref::Type::builtin(type_ref::Builtin::F64)])?;
    }
    builders(builder, "widget::Image", "scale", [type_ref::Type::builtin(type_ref::Builtin::F64)])?;
    builders(
        builder,
        "widget::Image",
        "filter_method",
        [type_ref::Type::registered("widget::image::FilterMethod")?],
    )?;
    for name in IMAGE_FILTER_METHODS {
        builder.item(item::Definition::associated_value(
            format!("widget::image::FilterMethod::{name}"),
            type_ref::Type::registered("widget::image::FilterMethod")?,
        )?)?;
    }
    for name in CONTENT_FITS {
        builder.item(item::Definition::associated_value(
            format!("ContentFit::{name}"),
            type_ref::Type::registered("ContentFit")?,
        )?)?;
    }

    let mut data = signature::Builder::new();
    data.parameter(string());
    data.output(type_ref::Type::result(
        type_ref::Type::registered("widget::qr_code::Data")?,
        type_ref::Type::registered("widget::qr_code::Error")?,
    ));
    builder.item(item::Definition::associated_function("widget::qr_code::Data::new", data.finish())?)?;
    let mut qr_code = signature::Builder::new();
    qr_code.parameter(type_ref::Type::registered("widget::qr_code::Data")?);
    qr_code.output(type_ref::Type::registered("widget::QRCode")?);
    builder.item(item::Definition::function("widget::qr_code", qr_code.finish())?)?;
    pixels_builders(builder, "widget::QRCode", "cell_size")?;
    pixels_builders(builder, "widget::QRCode", "total_size")?;
    Ok(())
}

/// iced's `span(text)`, `rich_text(spans)` and `rich_text![..]`. A span's
/// `Link` is any value; `on_link_click` maps a clicked link to a Message.
fn declare_rich_text(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let link = || type_ref::Type::parameter("Link");
    let span = || type_ref::Type::applied("widget::text::Span", [type_ref::Type::parameter("Link")?]);
    let rich = || {
        type_ref::Type::applied(
            "widget::text::Rich",
            [type_ref::Type::parameter("Link")?, type_ref::Type::parameter("Message")?],
        )
    };
    let mut function = signature::Builder::new();
    function.type_parameter("Link", &[])?;
    function.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    function.output(span()?);
    builder.item(item::Definition::function("widget::span", function.finish())?)?;
    let span_builder = |input: type_ref::Type| -> Result<signature::Definition, host::Error> {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Link", &[])?;
        signature.parameter(input);
        signature.output(span()?);
        Ok(signature.finish())
    };
    let color = || type_ref::Type::registered("Color");
    let font = || type_ref::Type::registered("Font");
    let bool_ = || type_ref::Type::builtin(type_ref::Builtin::Bool);
    let mut span_inputs = vec![
        ("color", color()?),
        ("color_maybe", type_ref::Type::option(color()?)),
        ("font", font()?),
        ("font_maybe", type_ref::Type::option(font()?)),
        ("link", link()?),
        ("link_maybe", type_ref::Type::option(link()?)),
        ("underline", bool_()),
        ("strikethrough", bool_()),
    ];
    span_inputs.extend(PIXELS.map(|pixels| ("size", type_ref::Type::builtin(pixels))));
    for (name, input) in span_inputs {
        builder.item(item::Definition::method(
            format!("widget::text::Span::{name}"),
            receiver::Mode::Owned,
            span_builder(input)?,
        )?)?;
    }

    let rich_signature = |input: Option<type_ref::Type>| -> Result<signature::Definition, host::Error> {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Link", &[])?;
        signature.type_parameter("Message", &[])?;
        if let Some(input) = input {
            signature.parameter(input);
        }
        signature.output(rich()?);
        Ok(signature.finish())
    };
    builder.item(item::Definition::function(
        "widget::rich_text",
        rich_signature(Some(type_ref::Type::slice(span()?)))?,
    )?)?;
    builder.item(item::Definition::intrinsic_macro(
        "widget::rich_text",
        rich_signature(Some(span()?))?,
    )?)?;
    let mut rich_inputs = vec![
        ("color", color()?),
        ("color_maybe", type_ref::Type::option(color()?)),
        ("font", font()?),
        (
            "on_link_click",
            type_ref::Type::callable(link()?, type_ref::Type::parameter("Message")?),
        ),
    ];
    rich_inputs.extend(PIXELS.map(|pixels| ("size", type_ref::Type::builtin(pixels))));
    for name in ["width", "height"] {
        rich_inputs.push((name, type_ref::Type::registered("Length")?));
        rich_inputs.extend(PIXELS.map(|pixels| (name, type_ref::Type::builtin(pixels))));
    }
    for (name, input) in rich_inputs {
        builder.item(item::Definition::method(
            format!("widget::text::Rich::{name}"),
            receiver::Mode::Owned,
            rich_signature(Some(input))?,
        )?)?;
    }
    Ok(())
}

/// iced's `canvas::Program`, as script types implement it: its `Theme`
/// and `Renderer` are iced's, and `update` and `mouse_interaction` have
/// iced's defaults.
const CANVAS_PROGRAM: &str = "pub trait Program<Message> {
    type State: Default;
    fn update(&self, state: &mut Self::State, event: &iced::Event, bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> Option<iced::widget::canvas::Action<Message>> { None }
    fn draw(&self, state: &Self::State, renderer: &iced::widget::Renderer, theme: &iced::Theme, bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> Vec<iced::widget::canvas::Geometry>;
    fn mouse_interaction(&self, state: &Self::State, bounds: iced::Rectangle, cursor: iced::mouse::Cursor) -> iced::mouse::Interaction { iced::mouse::Interaction::None }
}";

/// iced's `canvas`: a script `canvas::Program` drawn with `Frame`, `Path`,
/// `Stroke`, `Fill`, `Text` and `Cache`, and the same names under
/// `advanced::graphics::geometry`.
fn declare_canvas(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let ty = type_ref::Type::registered;
    let f64_ = || type_ref::Type::builtin(type_ref::Builtin::F64);
    let unit = || -> Result<type_ref::Type, host::Error> { Ok(type_ref::Type::unit()) };
    let message = || type_ref::Type::parameter("Message");
    let signature = |inputs: Vec<type_ref::Type>, output: type_ref::Type| {
        let mut signature = signature::Builder::new();
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        signature.finish()
    };
    let generic = |inputs: Vec<type_ref::Type>, output: type_ref::Type| -> Result<_, host::Error> {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Message", &[])?;
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        Ok(signature.finish())
    };

    for module in [
        "widget::canvas",
        "widget::canvas::path",
        "widget::canvas::stroke",
        "widget::canvas::fill",
        "advanced",
        "advanced::graphics",
        "advanced::graphics::geometry",
    ] {
        builder.module(module)?;
    }
    builder.private_type::<marker::CanvasProgram>("widget::canvas::Instance", &["Message"], &[])?;
    builder.implementable_trait::<marker::CanvasTrait>(
        "widget::canvas::Program",
        &["Message"],
        CANVAS_PROGRAM,
    )?;
    builder.project("widget::canvas::Program", "widget::canvas::Instance", &["Message"])?;
    builder.type_::<marker::Canvas>("widget::Canvas", &["Message"], &[])?;
    builder.project("widget::Canvas", "Element", &["Message"])?;
    builder.alias("widget::canvas::Canvas", "widget::Canvas")?;
    builder.type_::<marker::CanvasAction>("widget::canvas::Action", &["Message"], &[])?;
    builder.alias("widget::canvas::Event", "Event")?;
    let cloned = &[capability::Kind::Clone, capability::Kind::Send, capability::Kind::Static];
    let debug = &[
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Send,
        capability::Kind::Static,
    ];
    let copied_default = &[
        capability::Kind::Default,
        capability::Kind::Debug,
        capability::Kind::Clone,
        capability::Kind::Copy,
        capability::Kind::Send,
        capability::Kind::Static,
    ];
    builder.type_::<iced::widget::canvas::Path>("widget::canvas::Path", &[], debug)?;
    builder.type_::<marker::PathBuilder>("widget::canvas::path::Builder", &[], cloned)?;
    builder.type_::<marker::Frame>("widget::canvas::Frame", &[], cloned)?;
    builder.type_::<marker::Geometry>("widget::canvas::Geometry", &[], debug)?;
    builder.value_type::<marker::CacheGroup>(
        "widget::canvas::Group",
        &[],
        OWNED_VALUE_CAPABILITIES,
    )?;
    builder.default_type::<marker::Cache>(
        "widget::canvas::Cache",
        &[],
        &[
            capability::Kind::Default,
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.default_type::<iced::widget::canvas::Stroke<'static>>("widget::canvas::Stroke", &[], copied_default)?;
    builder.default_type::<iced::widget::canvas::Fill>("widget::canvas::Fill", &[], copied_default)?;
    builder.value_type::<iced::widget::canvas::Style>("widget::canvas::Style", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.value_type::<iced::widget::canvas::fill::Rule>(
        "widget::canvas::fill::Rule",
        &[],
        OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.default_type::<iced::widget::canvas::LineCap>("widget::canvas::LineCap", &[], copied_default)?;
    builder.default_type::<iced::widget::canvas::LineJoin>("widget::canvas::LineJoin", &[], copied_default)?;
    builder.default_type::<iced::widget::canvas::Text>(
        "widget::canvas::Text",
        &[],
        &[
            capability::Kind::Default,
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    builder.default_value_type::<iced::widget::text::Alignment>(
        "widget::text::Alignment",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<iced::mouse::Cursor>("mouse::Cursor", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.alias("widget::canvas::stroke::Style", "widget::canvas::Style")?;
    builder.alias("widget::canvas::fill::Style", "widget::canvas::Style")?;
    builder.alias("widget::canvas::stroke::Stroke", "widget::canvas::Stroke")?;
    builder.alias("widget::canvas::fill::Fill", "widget::canvas::Fill")?;
    builder.alias("widget::canvas::path::Path", "widget::canvas::Path")?;
    for name in ["Path", "Frame", "Cache", "Stroke", "Fill", "Style", "Text", "LineCap", "LineJoin"] {
        builder.alias(
            format!("advanced::graphics::geometry::{name}"),
            format!("widget::canvas::{name}"),
        )?;
    }
    builder.alias("advanced::graphics::geometry::Renderer", "widget::Renderer")?;
    for module in ["path", "stroke", "fill"] {
        builder.module(format!("advanced::graphics::geometry::{module}"))?;
    }
    builder.alias("advanced::graphics::geometry::path::Builder", "widget::canvas::path::Builder")?;
    builder.alias("advanced::graphics::geometry::stroke::Style", "widget::canvas::Style")?;
    builder.alias("advanced::graphics::geometry::fill::Rule", "widget::canvas::fill::Rule")?;

    // `canvas(program)` and `Canvas::new(program)`, with its size.
    let instance = || type_ref::Type::applied("widget::canvas::Instance", [message()?]);
    let canvas = || type_ref::Type::applied("widget::Canvas", [message()?]);
    builder.item(item::Definition::function(
        "widget::canvas",
        generic(vec![instance()?], canvas()?)?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "widget::Canvas::new",
        generic(vec![instance()?], canvas()?)?,
    )?)?;
    length_builders(builder, "widget::Canvas", &["width", "height"])?;
    // A program's `Instance`: its value, `update`, `draw` and
    // `mouse_interaction`, and the default of its State.
    let mut new = signature::Builder::new();
    for name in ["C", "S", "Message"] {
        new.type_parameter(name, &[])?;
    }
    let parameter = |name: &str| type_ref::Type::parameter(name);
    new.parameter(parameter("C")?);
    new.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([
            parameter("C")?,
            parameter("S")?,
            ty("Event")?,
            ty("Rectangle")?,
            ty("mouse::Cursor")?,
        ])?,
        type_ref::Type::option(type_ref::Type::applied("widget::canvas::Action", [message()?])?),
    ));
    new.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([
            parameter("C")?,
            parameter("S")?,
            ty("widget::Renderer")?,
            ty("Theme")?,
            ty("Rectangle")?,
            ty("mouse::Cursor")?,
        ])?,
        type_ref::Type::slice(ty("widget::canvas::Geometry")?),
    ));
    new.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([
            parameter("C")?,
            parameter("S")?,
            ty("Rectangle")?,
            ty("mouse::Cursor")?,
        ])?,
        ty("mouse::Interaction")?,
    ));
    new.parameter(parameter("S")?);
    new.output(instance()?);
    builder.item(item::Definition::associated_function(
        "widget::canvas::Instance::new",
        new.finish(),
    )?)?;

    // `canvas::Action::publish(message)` and kin.
    let action = || type_ref::Type::applied("widget::canvas::Action", [message()?]);
    builder.item(item::Definition::associated_function(
        "widget::canvas::Action::publish",
        generic(vec![message()?], action()?)?,
    )?)?;
    for name in ["none", "capture", "request_redraw"] {
        builder.item(item::Definition::associated_function(
            format!("widget::canvas::Action::{name}"),
            generic(Vec::new(), action()?)?,
        )?)?;
    }
    builder.item(item::Definition::method(
        "widget::canvas::Action::and_capture",
        receiver::Mode::Owned,
        generic(Vec::new(), action()?)?,
    )?)?;

    // `Path::circle(center, radius)` and kin, and `Path::new(|b| ..)`.
    let path = || ty("widget::canvas::Path");
    let builder_ty = || ty("widget::canvas::path::Builder");
    builder.item(item::Definition::associated_function(
        "widget::canvas::Path::new",
        signature(vec![type_ref::Type::callable(builder_ty()?, unit()?)], path()?),
    )?)?;
    for (name, inputs) in [
        ("circle", vec![ty("Point")?, f64_()]),
        ("line", vec![ty("Point")?, ty("Point")?]),
        ("rectangle", vec![ty("Point")?, ty("Size")?]),
        ("rounded_rectangle", vec![ty("Point")?, ty("Size")?, f64_()]),
    ] {
        builder.item(item::Definition::associated_function(
            format!("widget::canvas::Path::{name}"),
            signature(inputs, path()?),
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "widget::canvas::path::Builder::new",
        signature(Vec::new(), builder_ty()?),
    )?)?;
    for (name, inputs) in [
        ("move_to", vec![ty("Point")?]),
        ("line_to", vec![ty("Point")?]),
        ("circle", vec![ty("Point")?, f64_()]),
        ("rectangle", vec![ty("Point")?, ty("Size")?]),
        ("rounded_rectangle", vec![ty("Point")?, ty("Size")?, f64_()]),
        ("arc_to", vec![ty("Point")?, ty("Point")?, f64_()]),
        ("quadratic_curve_to", vec![ty("Point")?, ty("Point")?]),
        ("bezier_curve_to", vec![ty("Point")?, ty("Point")?, ty("Point")?]),
        ("close", Vec::new()),
    ] {
        builder.item(item::Definition::method(
            format!("widget::canvas::path::Builder::{name}"),
            receiver::Mode::Shared,
            signature(inputs, unit()?),
        )?)?;
    }
    builder.item(item::Definition::method(
        "widget::canvas::path::Builder::build",
        receiver::Mode::Owned,
        signature(Vec::new(), path()?),
    )?)?;

    // `Frame::new(renderer, size)`, its drawing and its transforms.
    let frame = || ty("widget::canvas::Frame");
    let geometry = || ty("widget::canvas::Geometry");
    let stroke = || ty("widget::canvas::Stroke");
    let fill = || ty("widget::canvas::Fill");
    builder.item(item::Definition::associated_function(
        "widget::canvas::Frame::new",
        signature(vec![ty("widget::Renderer")?, ty("Size")?], frame()?),
    )?)?;
    let mut frame_methods = vec![
        ("width", Vec::new(), f64_()),
        ("height", Vec::new(), f64_()),
        ("size", Vec::new(), ty("Size")?),
        ("center", Vec::new(), ty("Point")?),
        ("stroke", vec![path()?, stroke()?], unit()?),
        ("stroke_rectangle", vec![ty("Point")?, ty("Size")?, stroke()?], unit()?),
        ("fill_text", vec![ty("widget::canvas::Text")?], unit()?),
        ("fill_text", vec![type_ref::Type::builtin(type_ref::Builtin::String)], unit()?),
        ("translate", vec![ty("Vector")?], unit()?),
        ("rotate", vec![f64_()], unit()?),
        ("scale", vec![f64_()], unit()?),
        ("scale_nonuniform", vec![ty("Vector")?], unit()?),
        ("push_transform", Vec::new(), unit()?),
        ("pop_transform", Vec::new(), unit()?),
    ];
    // `impl Into<Fill>`: a `Fill`, or a `Color` filled solid.
    for paint in [fill()?, ty("Color")?] {
        frame_methods.push(("fill", vec![path()?, paint.clone()], unit()?));
        frame_methods.push(("fill_rectangle", vec![ty("Point")?, ty("Size")?, paint], unit()?));
    }
    for (name, inputs, output) in frame_methods {
        builder.item(item::Definition::method(
            format!("widget::canvas::Frame::{name}"),
            receiver::Mode::Shared,
            signature(inputs, output),
        )?)?;
    }
    let mut with_save = signature::Builder::new();
    with_save.type_parameter("R", &[])?;
    with_save.parameter(type_ref::Type::callable(frame()?, type_ref::Type::parameter("R")?));
    with_save.output(type_ref::Type::parameter("R")?);
    builder.item(item::Definition::method(
        "widget::canvas::Frame::with_save",
        receiver::Mode::Shared,
        with_save.finish(),
    )?)?;
    builder.item(item::Definition::method(
        "widget::canvas::Frame::into_geometry",
        receiver::Mode::Owned,
        signature(Vec::new(), geometry()?),
    )?)?;

    // `Cache::new()`, `clear()` and `draw(renderer, size, |frame| ..)`.
    let cache = || ty("widget::canvas::Cache");
    for name in ["new", "default"] {
        builder.item(item::Definition::associated_function(
            format!("widget::canvas::Cache::{name}"),
            signature(Vec::new(), cache()?),
        )?)?;
    }
    // `Group::unique()` and `Cache::with_group(group)`: iced batches the
    // caches of a group, which Rawr draws one by one.
    builder.item(item::Definition::associated_function(
        "widget::canvas::Group::unique",
        signature(Vec::new(), ty("widget::canvas::Group")?),
    )?)?;
    builder.item(item::Definition::associated_function(
        "widget::canvas::Cache::with_group",
        signature(vec![ty("widget::canvas::Group")?], cache()?),
    )?)?;
    builder.item(item::Definition::method(
        "widget::canvas::Cache::clear",
        receiver::Mode::Shared,
        signature(Vec::new(), unit()?),
    )?)?;
    builder.item(item::Definition::method(
        "widget::canvas::Cache::draw",
        receiver::Mode::Shared,
        signature(
            vec![
                ty("widget::Renderer")?,
                ty("Size")?,
                type_ref::Type::callable(frame()?, unit()?),
            ],
            geometry()?,
        ),
    )?)?;
    // What `draw` reads and keeps: Rawr runs its closure itself.
    builder.item(item::Definition::method(
        "widget::canvas::Cache::__rawr_cached",
        receiver::Mode::Shared,
        signature(vec![ty("Size")?], type_ref::Type::option(geometry()?)),
    )?)?;
    builder.item(item::Definition::method(
        "widget::canvas::Cache::__rawr_store",
        receiver::Mode::Shared,
        signature(vec![geometry()?], geometry()?),
    )?)?;

    // `Stroke { width, style, line_cap, .. }` and its `with_*` builders.
    builder.item(item::Definition::associated_function(
        "widget::canvas::Stroke::default",
        signature(Vec::new(), stroke()?),
    )?)?;
    for (name, field) in [
        ("width", f64_()),
        ("style", ty("widget::canvas::Style")?),
        ("line_cap", ty("widget::canvas::LineCap")?),
        ("line_join", ty("widget::canvas::LineJoin")?),
    ] {
        builder.item(item::Definition::method(
            format!("widget::canvas::Stroke::{name}"),
            receiver::Mode::Shared,
            signature(Vec::new(), field.clone()),
        )?)?;
        builder.item(item::Definition::method(
            format!("widget::canvas::Stroke::{name}"),
            receiver::Mode::Owned,
            signature(vec![field], stroke()?),
        )?)?;
    }
    for (name, input) in [
        ("with_width", f64_()),
        ("with_color", ty("Color")?),
        ("with_line_cap", ty("widget::canvas::LineCap")?),
        ("with_line_join", ty("widget::canvas::LineJoin")?),
    ] {
        builder.item(item::Definition::method(
            format!("widget::canvas::Stroke::{name}"),
            receiver::Mode::Owned,
            signature(vec![input], stroke()?),
        )?)?;
    }
    // `Fill { style, rule }`, and `Style::Solid(color)`.
    builder.item(item::Definition::associated_function(
        "widget::canvas::Fill::default",
        signature(Vec::new(), fill()?),
    )?)?;
    for (name, field) in [
        ("style", ty("widget::canvas::Style")?),
        ("rule", ty("widget::canvas::fill::Rule")?),
    ] {
        builder.item(item::Definition::method(
            format!("widget::canvas::Fill::{name}"),
            receiver::Mode::Shared,
            signature(Vec::new(), field.clone()),
        )?)?;
        builder.item(item::Definition::method(
            format!("widget::canvas::Fill::{name}"),
            receiver::Mode::Owned,
            signature(vec![field], fill()?),
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "widget::canvas::Style::Solid",
        signature(vec![ty("Color")?], ty("widget::canvas::Style")?),
    )?)?;
    for name in ["NonZero", "EvenOdd"] {
        builder.item(item::Definition::associated_value(
            format!("widget::canvas::fill::Rule::{name}"),
            ty("widget::canvas::fill::Rule")?,
        )?)?;
    }
    for name in ["Butt", "Square", "Round"] {
        builder.item(item::Definition::associated_value(
            format!("widget::canvas::LineCap::{name}"),
            ty("widget::canvas::LineCap")?,
        )?)?;
    }
    for name in ["Miter", "Round", "Bevel"] {
        builder.item(item::Definition::associated_value(
            format!("widget::canvas::LineJoin::{name}"),
            ty("widget::canvas::LineJoin")?,
        )?)?;
    }
    for name in ["LineCap", "LineJoin"] {
        let owner = format!("widget::canvas::{name}");
        builder.item(item::Definition::associated_function(
            format!("{owner}::default"),
            signature(Vec::new(), type_ref::Type::registered(owner)?),
        )?)?;
    }

    // `canvas::Text { content, position, color, size, shaping, .. }`.
    let text = || ty("widget::canvas::Text");
    builder.item(item::Definition::associated_function(
        "widget::canvas::Text::default",
        signature(Vec::new(), text()?),
    )?)?;
    for (name, field) in [
        ("content", type_ref::Type::builtin(type_ref::Builtin::String)),
        ("position", ty("Point")?),
        ("max_width", f64_()),
        ("color", ty("Color")?),
        ("size", f64_()),
        ("font", ty("Font")?),
        ("align_x", ty("widget::text::Alignment")?),
        ("align_y", ty("alignment::Vertical")?),
        ("shaping", ty("widget::text::Shaping")?),
    ] {
        builder.item(item::Definition::method(
            format!("widget::canvas::Text::{name}"),
            receiver::Mode::Shared,
            signature(Vec::new(), field.clone()),
        )?)?;
        builder.item(item::Definition::method(
            format!("widget::canvas::Text::{name}"),
            receiver::Mode::Owned,
            signature(vec![field], text()?),
        )?)?;
    }
    for name in ["Default", "Left", "Center", "Right", "Justified"] {
        builder.item(item::Definition::associated_value(
            format!("widget::text::Alignment::{name}"),
            ty("widget::text::Alignment")?,
        )?)?;
    }

    // `mouse::Cursor`, as a canvas reads it.
    builder.item(item::Definition::associated_value(
        "mouse::Cursor::Unavailable",
        ty("mouse::Cursor")?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "mouse::Cursor::Available",
        signature(vec![ty("Point")?], ty("mouse::Cursor")?),
    )?)?;
    for (name, inputs, output) in [
        ("position", Vec::new(), type_ref::Type::option(ty("Point")?)),
        ("position_in", vec![ty("Rectangle")?], type_ref::Type::option(ty("Point")?)),
        ("position_over", vec![ty("Rectangle")?], type_ref::Type::option(ty("Point")?)),
        ("position_from", vec![ty("Point")?], type_ref::Type::option(ty("Point")?)),
        ("is_over", vec![ty("Rectangle")?], type_ref::Type::builtin(type_ref::Builtin::Bool)),
    ] {
        builder.item(item::Definition::method(
            format!("mouse::Cursor::{name}"),
            receiver::Mode::Shared,
            signature(inputs, output),
        )?)?;
    }

    // What a canvas reads of its bounds: `bounds.size()`, `center()` and
    // `contains(point)`, and `Point::ORIGIN`.
    for (name, inputs, output) in [
        ("size", Vec::new(), ty("Size")?),
        ("center", Vec::new(), ty("Point")?),
        ("position", Vec::new(), ty("Point")?),
        ("contains", vec![ty("Point")?], type_ref::Type::builtin(type_ref::Builtin::Bool)),
    ] {
        builder.item(item::Definition::method(
            format!("Rectangle::{name}"),
            receiver::Mode::Shared,
            signature(inputs, output),
        )?)?;
    }
    builder.item(item::Definition::method(
        "Point::distance",
        receiver::Mode::Shared,
        signature(vec![ty("Point")?], f64_()),
    )?)?;
    builder.item(item::Definition::associated_value("Point::ORIGIN", ty("Point")?)?)?;
    builder.item(item::Definition::associated_function(
        "mouse::Interaction::default",
        signature(Vec::new(), ty("mouse::Interaction")?),
    )?)?;
    Ok(())
}

/// iced's runtime events as subscriptions: `event::listen()`,
/// `keyboard::listen()`, `window::frames()` and `time::every(duration)`,
/// with `Subscription::filter_map`. `keyboard::Event` matches as in Rust
/// (`keyboard::Event::KeyPressed { key, modifiers, .. }`); a `Key` is
/// compared with `Key::Named(key::Named::Space)` or `Key::Character("r")`.
fn declare_events(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let stream = |path: &str,
                  inputs: &[type_ref::Type],
                  output: type_ref::Type|
     -> Result<item::Definition, host::Error> {
        let mut signature = signature::Builder::new();
        for input in inputs {
            signature.parameter(input.clone());
        }
        signature.output(output);
        item::Definition::stream_operation(path, signature.finish(), host::stream::Version::new(1))
    };
    builder.item(stream("event::listen", &[], type_ref::Type::registered("Event")?)?)?;
    // iced's `event::listen_with(f)`: every runtime event but redraws,
    // with its status and window, filtered through `f`.
    builder.item(stream(
        "event::listen_with",
        &[],
        type_ref::Type::tuple([
            type_ref::Type::registered("Event")?,
            type_ref::Type::registered("event::Status")?,
            type_ref::Type::registered("window::Id")?,
        ])?,
    )?)?;
    for name in ["Ignored", "Captured"] {
        builder.item(item::Definition::associated_value(
            format!("event::Status::{name}"),
            type_ref::Type::registered("event::Status")?,
        )?)?;
    }
    // iced's `event::listen_url()`: the URLs macOS hands the application.
    builder.item(stream(
        "event::listen_url",
        &[],
        type_ref::Type::builtin(type_ref::Builtin::String),
    )?)?;
    builder.item(stream(
        "keyboard::listen",
        &[],
        type_ref::Type::registered("keyboard::Event")?,
    )?)?;
    builder.item(stream("window::frames", &[], type_ref::Type::registered("time::Instant")?)?)?;
    builder.item(stream(
        "time::every",
        &[type_ref::Type::registered("time::Duration")?],
        type_ref::Type::registered("time::Instant")?,
    )?)?;

    // iced's `Subscription::batch(..)` and `Task::batch(..)`: any `Vec` or
    // array of them.
    for owner in ["Subscription", "Task"] {
        let mut batch = signature::Builder::new();
        batch.type_parameter("Message", &[])?;
        let plan = type_ref::Type::applied(owner, [type_ref::Type::parameter("Message")?])?;
        batch.parameter(type_ref::Type::slice(plan.clone()));
        batch.output(plan);
        builder.item(item::Definition::associated_function(
            format!("{owner}::batch"),
            batch.finish(),
        )?)?;
    }
    let mut filter_map = signature::Builder::new();
    filter_map.type_parameter("Message", &[])?;
    filter_map.type_parameter("Parent", &[])?;
    filter_map.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("Message")?,
        type_ref::Type::option(type_ref::Type::parameter("Parent")?),
    ));
    filter_map.output(type_ref::Type::applied(
        "Subscription",
        [type_ref::Type::parameter("Parent")?],
    )?);
    builder.item(item::Definition::method(
        "Subscription::filter_map",
        receiver::Mode::Owned,
        filter_map.finish(),
    )?)?;

    // `keyboard::Event`'s variants, as a facade enum, and their fields.
    let bool_reader = || -> Result<signature::Definition, host::Error> {
        let mut signature = signature::Builder::new();
        signature.output(type_ref::Type::builtin(type_ref::Builtin::Bool));
        Ok(signature.finish())
    };
    for variant in ["KeyPressed", "KeyReleased", "ModifiersChanged"] {
        builder.item(item::Definition::method(
            format!("keyboard::Event::{variant}"),
            receiver::Mode::Shared,
            bool_reader()?,
        )?)?;
    }
    for (field, output) in [
        ("key", type_ref::Type::registered("keyboard::Key")?),
        ("modified_key", type_ref::Type::registered("keyboard::Key")?),
        ("modifiers", type_ref::Type::registered("keyboard::Modifiers")?),
        ("repeat", type_ref::Type::builtin(type_ref::Builtin::Bool)),
    ] {
        let mut reader = signature::Builder::new();
        reader.output(output);
        builder.item(item::Definition::method(
            format!("keyboard::Event::{field}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
    }
    let mut named = signature::Builder::new();
    named.parameter(type_ref::Type::registered("keyboard::key::Named")?);
    named.output(type_ref::Type::registered("keyboard::Key")?);
    builder.item(item::Definition::associated_function("keyboard::Key::Named", named.finish())?)?;
    let mut character = signature::Builder::new();
    character.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    character.output(type_ref::Type::registered("keyboard::Key")?);
    builder.item(item::Definition::associated_function(
        "keyboard::Key::Character",
        character.finish(),
    )?)?;
    builder.item(item::Definition::associated_value(
        "keyboard::Key::Unidentified",
        type_ref::Type::registered("keyboard::Key")?,
    )?)?;
    // `key.as_ref()`: iced's `Key<&str>`, the same key here.
    builder.item(item::Definition::method(
        "keyboard::Key::as_ref",
        receiver::Mode::Shared,
        output_signature("keyboard::Key")?,
    )?)?;
    for name in KEY_NAMES {
        builder.item(item::Definition::associated_value(
            format!("keyboard::key::Named::{name}"),
            type_ref::Type::registered("keyboard::key::Named")?,
        )?)?;
    }
    for name in ["shift", "control", "alt", "logo", "command", "macos_command", "jump"] {
        builder.item(item::Definition::method(
            format!("keyboard::Modifiers::{name}"),
            receiver::Mode::Shared,
            bool_reader()?,
        )?)?;
    }
    Ok(())
}

/// `iced::Event`, `window::Event`, `mouse::Event` and `touch::Event` as
/// facade enums, matched as in Rust: `Event::Keyboard(keyboard::Event::KeyPressed
/// { key, .. })`. A unit or struct variant is a shared `bool` reader named for
/// it; a tuple variant of one field is a shared reader named for it that
/// returns the field when the value is that variant. Struct fields are shared
/// readers. `Key::Named(..)` and `Key::Character(..)` are tuple variants too.
fn declare_event_variants(builder: &mut facade::Builder) -> Result<(), host::Error> {
    fn reader(
        builder: &mut facade::Builder,
        path: String,
        output: type_ref::Type,
    ) -> Result<(), host::Error> {
        let mut signature = signature::Builder::new();
        signature.output(output);
        builder.item(item::Definition::method(path, receiver::Mode::Shared, signature.finish())?)?;
        Ok(())
    }
    let bool_ = || type_ref::Type::builtin(type_ref::Builtin::Bool);
    let some = |path: &str| -> Result<type_ref::Type, host::Error> {
        Ok(type_ref::Type::option(type_ref::Type::registered(path)?))
    };
    // A tuple variant whose field the facade does not name is tested only:
    // `Event::InputMethod(_)`.
    for (variant, payload) in [
        ("Keyboard", Some("keyboard::Event")),
        ("Mouse", Some("mouse::Event")),
        ("Window", Some("window::Event")),
        ("Touch", Some("touch::Event")),
        ("InputMethod", None),
        ("Clipboard", None),
        ("Waken", None),
    ] {
        let output = match payload {
            Some(payload) => some(payload)?,
            None => bool_(),
        };
        reader(builder, format!("Event::{variant}"), output)?;
    }
    for (variant, payload) in [
        ("Opened", None),
        ("Closed", None),
        ("Moved", Some(some("Point")?)),
        ("Resized", Some(some("Size")?)),
        ("Rescaled", Some(type_ref::Type::option(type_ref::Type::builtin(type_ref::Builtin::F32)))),
        ("RedrawRequested", Some(some("time::Instant")?)),
        ("CloseRequested", None),
        ("Focused", None),
        ("Unfocused", None),
        ("FileHovered", None),
        ("FileDropped", None),
        ("FilesHoveredLeft", None),
    ] {
        reader(builder, format!("window::Event::{variant}"), payload.unwrap_or_else(bool_))?;
    }
    reader(
        builder,
        "window::Event::position".into(),
        type_ref::Type::option(type_ref::Type::registered("Point")?),
    )?;
    reader(builder, "window::Event::size".into(), type_ref::Type::registered("Size")?)?;
    reader(
        builder,
        "window::Event::scale_factor".into(),
        type_ref::Type::builtin(type_ref::Builtin::F64),
    )?;
    for (variant, payload) in [
        ("CursorEntered", None),
        ("CursorLeft", None),
        ("CursorMoved", None),
        ("ButtonPressed", Some(some("mouse::Button")?)),
        ("ButtonReleased", Some(some("mouse::Button")?)),
        ("WheelScrolled", None),
    ] {
        reader(builder, format!("mouse::Event::{variant}"), payload.unwrap_or_else(bool_))?;
    }
    reader(builder, "mouse::Event::position".into(), type_ref::Type::registered("Point")?)?;
    for name in ["Left", "Right", "Middle", "Back", "Forward"] {
        builder.item(item::Definition::associated_value(
            format!("mouse::Button::{name}"),
            type_ref::Type::registered("mouse::Button")?,
        )?)?;
    }
    for variant in ["FingerPressed", "FingerMoved", "FingerLifted", "FingerLost"] {
        reader(builder, format!("touch::Event::{variant}"), bool_())?;
    }
    reader(builder, "touch::Event::position".into(), type_ref::Type::registered("Point")?)?;
    // `Key::Named(named)` and `Key::Character(c)` in a pattern: beside
    // their constructors, the readers of their fields.
    reader(builder, "keyboard::Key::Named".into(), some("keyboard::key::Named")?)?;
    reader(
        builder,
        "keyboard::Key::Character".into(),
        type_ref::Type::option(type_ref::Type::builtin(type_ref::Builtin::String)),
    )?;
    Ok(())
}

/// The `keyboard::key::Named` keys the facade names.
pub(crate) const KEY_NAMES: [&str; 40] = [
    "Alt", "Control", "Shift", "Super", "Meta", "Enter", "Tab", "Space", "ArrowDown", "ArrowLeft",
    "ArrowRight", "ArrowUp", "End", "Home", "PageDown", "PageUp", "Backspace", "Delete", "Insert",
    "Escape", "CapsLock", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    "Copy", "Cut", "Paste", "Undo", "Redo", "ContextMenu", "PrintScreen",
];

/// iced's `image::FilterMethod` variants.
pub(crate) const IMAGE_FILTER_METHODS: [&str; 2] = ["Linear", "Nearest"];

/// iced's `ContentFit` variants.
pub(crate) const CONTENT_FITS: [&str; 5] = ["Contain", "Cover", "Fill", "None", "ScaleDown"];

/// `MouseArea`'s builders that take a Message.
pub(crate) const MOUSE_AREA_MESSAGES: [&str; 9] = [
    "on_press",
    "on_release",
    "on_double_click",
    "on_right_press",
    "on_right_release",
    "on_middle_press",
    "on_middle_release",
    "on_enter",
    "on_exit",
];

/// iced's own tasks: `exit()`, `window::latest()`, `window::close(id)`,
/// `operation::focus_next()`/`focus_previous()`, and `Task::and_then`.
/// `and_then` takes `impl Fn(T) -> Task<U>` on a `Task<Option<T>>`; the
/// facade checks the function, and the task's `Option` when it runs.
fn declare_native_tasks(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let task = |message: &str| -> Result<type_ref::Type, host::Error> {
        type_ref::Type::applied("Task", [type_ref::Type::parameter(message)?])
    };
    for name in [
        "exit",
        "widget::operation::focus_next",
        "widget::operation::focus_previous",
    ] {
        let mut signature = signature::Builder::new();
        signature.type_parameter("Message", &[])?;
        signature.output(task("Message")?);
        builder.item(item::Definition::function(name, signature.finish())?)?;
    }
    let mut latest = signature::Builder::new();
    latest.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::option(type_ref::Type::registered("window::Id")?)],
    )?);
    builder.item(item::Definition::function("window::latest", latest.finish())?)?;
    // `font::list()`: the system's font families.
    let mut list = signature::Builder::new();
    list.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::result(
            type_ref::Type::slice(type_ref::Type::registered("font::Family")?),
            type_ref::Type::registered("font::Error")?,
        )],
    )?);
    builder.item(item::Definition::function("font::list", list.finish())?)?;
    let mut scale_factor = signature::Builder::new();
    scale_factor.parameter(type_ref::Type::registered("window::Id")?);
    scale_factor.output(type_ref::Type::applied(
        "Task",
        [type_ref::Type::builtin(type_ref::Builtin::F64)],
    )?);
    builder.item(item::Definition::function(
        "window::scale_factor",
        scale_factor.finish(),
    )?)?;
    declare_operations(builder)?;
    let mut close = signature::Builder::new();
    close.type_parameter("Message", &[])?;
    close.parameter(type_ref::Type::registered("window::Id")?);
    close.output(task("Message")?);
    builder.item(item::Definition::function("window::close", close.finish())?)?;
    let mut and_then = signature::Builder::new();
    and_then.type_parameter("Message", &[])?;
    and_then.type_parameter("T", &[])?;
    and_then.type_parameter("Parent", &[])?;
    and_then.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("T")?,
        task("Parent")?,
    ));
    and_then.output(task("Parent")?);
    builder.item(item::Definition::method(
        "Task::and_then",
        receiver::Mode::Owned,
        and_then.finish(),
    )?)?;
    Ok(())
}

/// `widget::Id`, the `id` builders of `scrollable` and `text_input`, and
/// the operations that take one: `operation::focus` and
/// `operation::scrollable::{snap_to, snap_to_end, scroll_to, scroll_by}`
/// with their offsets and `operation::Animation`.
fn declare_operations(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let id = || type_ref::Type::registered("widget::Id");
    let mut new = signature::Builder::new();
    new.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    new.output(id()?);
    builder.item(item::Definition::associated_function("widget::Id::new", new.finish())?)?;
    builder.item(item::Definition::associated_function(
        "widget::Id::unique",
        output_signature("widget::Id")?,
    )?)?;
    // iced's `impl Into<Id>`: an `Id`, or a string.
    let ids = || -> Result<[type_ref::Type; 2], host::Error> {
        Ok([id()?, type_ref::Type::builtin(type_ref::Builtin::String)])
    };
    for owner in ["widget::Container", "widget::Scrollable", "widget::TextInput"] {
        builders(builder, owner, "id", ids()?)?;
    }
    for name in OPERATION_ANIMATIONS {
        builder.item(item::Definition::associated_value(
            format!("widget::operation::Animation::{name}"),
            type_ref::Type::registered("widget::operation::Animation")?,
        )?)?;
    }
    for offset in [
        "widget::scrollable::RelativeOffset",
        "widget::scrollable::AbsoluteOffset",
    ] {
        builder.item(item::Definition::associated_function(
            format!("{offset}::default"),
            output_signature(offset)?,
        )?)?;
        for field in ["x", "y"] {
            let mut reader = signature::Builder::new();
            reader.output(type_ref::Type::builtin(type_ref::Builtin::F64));
            builder.item(item::Definition::method(
                format!("{offset}::{field}"),
                receiver::Mode::Shared,
                reader.finish(),
            )?)?;
            let mut setter = signature::Builder::new();
            setter.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
            setter.output(type_ref::Type::registered(offset)?);
            builder.item(item::Definition::method(
                format!("{offset}::{field}"),
                receiver::Mode::Owned,
                setter.finish(),
            )?)?;
        }
    }
    for name in ["START", "END"] {
        builder.item(item::Definition::associated_value(
            format!("widget::scrollable::RelativeOffset::{name}"),
            type_ref::Type::registered("widget::scrollable::RelativeOffset")?,
        )?)?;
    }
    let task = || type_ref::Type::applied("Task", [type_ref::Type::parameter("Message")?]);
    let animation = || type_ref::Type::registered("widget::operation::Animation");
    for input in ids()? {
        let mut focus = signature::Builder::new();
        focus.type_parameter("Message", &[])?;
        focus.parameter(input);
        focus.output(task()?);
        builder.item(item::Definition::function("widget::operation::focus", focus.finish())?)?;
    }
    // `selector::find(id)`: the widget with that id, once iced lays it out.
    let target = || type_ref::Type::registered("widget::selector::Target");
    for input in ids()? {
        let mut find = signature::Builder::new();
        find.parameter(input);
        find.output(type_ref::Type::applied("Task", [type_ref::Type::option(target()?)])?);
        builder.item(item::Definition::function("widget::selector::find", find.finish())?)?;
    }
    for (name, output) in [
        ("bounds", type_ref::Type::registered("Rectangle")?),
        ("visible_bounds", type_ref::Type::option(type_ref::Type::registered("Rectangle")?)),
    ] {
        let mut reader = signature::Builder::new();
        reader.output(output);
        builder.item(item::Definition::method(
            format!("widget::selector::Target::{name}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
    }
    for (name, offset) in [
        ("snap_to", Some("widget::scrollable::RelativeOffset")),
        ("snap_to_end", None),
        ("scroll_to", Some("widget::scrollable::AbsoluteOffset")),
        ("scroll_by", Some("widget::scrollable::AbsoluteOffset")),
    ] {
        for input in ids()? {
            let mut signature = signature::Builder::new();
            signature.type_parameter("Message", &[])?;
            signature.parameter(input);
            if let Some(offset) = offset {
                signature.parameter(type_ref::Type::registered(offset)?);
            }
            signature.parameter(animation()?);
            signature.output(task()?);
            builder.item(item::Definition::function(
                format!("widget::operation::scrollable::{name}"),
                signature.finish(),
            )?)?;
        }
    }
    Ok(())
}

/// iced's `operation::Animation` variants.
pub(crate) const OPERATION_ANIMATIONS: [&str; 3] = ["Auto", "Instant", "Smooth"];

/// iced's `font` module: `Weight`, `Stretch`, `Style` and `Family`, and
/// `Font::new`, `Font::with_family` and the builders a struct literal such as
/// `Font { weight: font::Weight::Bold, ..Font::DEFAULT }` lowers to.
fn declare_font(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for (owner, variants) in [
        ("font::Weight", &FONT_WEIGHTS[..]),
        ("font::Stretch", &FONT_STRETCHES[..]),
        ("font::Style", &FONT_STYLES[..]),
        ("font::Family", &FONT_FAMILIES[..]),
    ] {
        for name in variants {
            builder.item(item::Definition::associated_value(
                format!("{owner}::{name}"),
                type_ref::Type::registered(owner)?,
            )?)?;
        }
    }
    // `Family::VARIANTS`, the standalone families.
    builder.item(item::Definition::associated_value(
        "font::Family::VARIANTS",
        type_ref::Type::slice(type_ref::Type::registered("font::Family")?),
    )?)?;
    let mut name = signature::Builder::new();
    name.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    name.output(type_ref::Type::registered("font::Family")?);
    builder.item(item::Definition::associated_function("font::Family::name", name.finish())?)?;
    let mut new = signature::Builder::new();
    new.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    new.output(type_ref::Type::registered("Font")?);
    builder.item(item::Definition::associated_function("Font::new", new.finish())?)?;
    let mut family = signature::Builder::new();
    family.parameter(type_ref::Type::registered("font::Family")?);
    family.output(type_ref::Type::registered("Font")?);
    builder.item(item::Definition::associated_function("Font::with_family", family.finish())?)?;
    for (name, input) in [
        ("weight", "font::Weight"),
        ("stretch", "font::Stretch"),
        ("style", "font::Style"),
        ("family", "font::Family"),
    ] {
        let mut method = signature::Builder::new();
        method.parameter(type_ref::Type::registered(input)?);
        method.output(type_ref::Type::registered("Font")?);
        builder.item(item::Definition::method(
            format!("Font::{name}"),
            receiver::Mode::Owned,
            method.finish(),
        )?)?;
        // `font.family`, read as a field.
        builder.item(item::Definition::method(
            format!("Font::{name}"),
            receiver::Mode::Shared,
            output_signature(input)?,
        )?)?;
    }
    Ok(())
}

/// iced's `scrollable::Anchor`, `Scrollbar` and `Direction`, written as
/// in Rust (`Direction::Both { horizontal, vertical }` is a struct
/// literal), the `Scroll` that `on_scroll` hands over with its `Viewport`
/// and `Source`, and the `direction`, `spacing` and `auto_scroll` builders.
fn declare_scrollable(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let ty = type_ref::Type::registered;
    let anchor = || ty("widget::scrollable::Anchor");
    let scrollbar = || ty("widget::scrollable::Scrollbar");
    let direction = || ty("widget::scrollable::Direction");
    let both = || ty("widget::scrollable::Direction::Both");
    for name in SCROLLABLE_ANCHORS {
        builder.item(item::Definition::associated_value(
            format!("widget::scrollable::Anchor::{name}"),
            anchor()?,
        )?)?;
    }
    for name in SCROLLABLE_SOURCES {
        builder.item(item::Definition::associated_value(
            format!("widget::scrollable::Source::{name}"),
            ty("widget::scrollable::Source")?,
        )?)?;
    }
    for owner in [
        "widget::scrollable::Anchor",
        "widget::scrollable::Scrollbar",
        "widget::scrollable::Direction",
        "widget::scrollable::Direction::Both",
    ] {
        builder.item(item::Definition::associated_function(
            format!("{owner}::default"),
            output_signature(owner)?,
        )?)?;
    }
    for name in ["new", "hidden"] {
        builder.item(item::Definition::associated_function(
            format!("widget::scrollable::Scrollbar::{name}"),
            output_signature("widget::scrollable::Scrollbar")?,
        )?)?;
    }
    let mut inputs = vec![("anchor", anchor()?)];
    for name in ["width", "margin", "scroller_width", "spacing", "padding"] {
        inputs.extend(PIXELS.map(|pixels| (name, type_ref::Type::builtin(pixels))));
    }
    for (name, input) in inputs {
        let mut setter = signature::Builder::new();
        setter.parameter(input).output(scrollbar()?);
        builder.item(item::Definition::method(
            format!("widget::scrollable::Scrollbar::{name}"),
            receiver::Mode::Owned,
            setter.finish(),
        )?)?;
    }
    for name in ["Vertical", "Horizontal"] {
        let mut variant = signature::Builder::new();
        variant.parameter(scrollbar()?).output(direction()?);
        builder.item(item::Definition::associated_function(
            format!("widget::scrollable::Direction::{name}"),
            variant.finish(),
        )?)?;
    }
    for field in ["horizontal", "vertical"] {
        let mut reader = signature::Builder::new();
        reader.output(scrollbar()?);
        builder.item(item::Definition::method(
            format!("widget::scrollable::Direction::Both::{field}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
        let mut setter = signature::Builder::new();
        setter.parameter(scrollbar()?).output(both()?);
        builder.item(item::Definition::method(
            format!("widget::scrollable::Direction::Both::{field}"),
            receiver::Mode::Owned,
            setter.finish(),
        )?)?;
    }
    // The literal is made the `Direction` by its `into` where it ends.
    builder.item(item::Definition::method(
        "widget::scrollable::Direction::Both::into",
        receiver::Mode::Owned,
        output_signature("widget::scrollable::Direction")?,
    )?)?;
    builders(builder, "widget::Scrollable", "direction", [direction()?])?;
    pixels_builders(builder, "widget::Scrollable", "spacing")?;
    builders(
        builder,
        "widget::Scrollable",
        "auto_scroll",
        [type_ref::Type::builtin(type_ref::Builtin::Bool)],
    )?;
    builder.item(item::Definition::method(
        "widget::Scrollable::on_scroll",
        receiver::Mode::Owned,
        message_mapper_signature("widget::Scrollable", ty("widget::scrollable::Scroll")?)?,
    )?)?;
    for (owner, field, output) in [
        ("widget::scrollable::Scroll", "viewport", "widget::scrollable::Viewport"),
        ("widget::scrollable::Scroll", "source", "widget::scrollable::Source"),
        ("widget::scrollable::Viewport", "relative_offset", "widget::scrollable::RelativeOffset"),
        ("widget::scrollable::Viewport", "absolute_offset", "widget::scrollable::AbsoluteOffset"),
    ] {
        builder.item(item::Definition::method(
            format!("{owner}::{field}"),
            receiver::Mode::Shared,
            output_signature(output)?,
        )?)?;
    }
    Ok(())
}

/// iced's `table(columns, rows)` and `table::column(header, view)`, whose
/// view makes each row's cell.
fn declare_table(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let parameter = |name: &str| type_ref::Type::parameter(name);
    let element = || type_ref::Type::applied("Element", [parameter("Message")?]);
    let column = || {
        type_ref::Type::applied("widget::table::Column", [parameter("T")?, parameter("Message")?])
    };
    let table = || type_ref::Type::applied("widget::Table", [parameter("Message")?]);
    let mut new = signature::Builder::new();
    new.type_parameter("T", &[])?;
    new.type_parameter("Message", &[])?;
    new.parameter(element()?);
    new.parameter(type_ref::Type::callable(parameter("T")?, element()?));
    new.output(column()?);
    builder.item(item::Definition::function("widget::table::column", new.finish())?)?;
    let mut new = signature::Builder::new();
    new.type_parameter("T", &[])?;
    new.type_parameter("Message", &[])?;
    new.parameter(type_ref::Type::slice(column()?));
    new.parameter(type_ref::Type::slice(parameter("T")?));
    new.output(table()?);
    builder.item(item::Definition::function("widget::table", new.finish())?)?;
    let mut inputs = vec![("width", type_ref::Type::registered("Length")?)];
    inputs.extend(PIXELS.map(|pixels| ("width", type_ref::Type::builtin(pixels))));
    for input in ["alignment::Horizontal", "Alignment"] {
        inputs.push(("align_x", type_ref::Type::registered(input)?));
    }
    for input in ["alignment::Vertical", "Alignment"] {
        inputs.push(("align_y", type_ref::Type::registered(input)?));
    }
    for (name, input) in inputs {
        let mut setter = signature::Builder::new();
        setter.type_parameter("T", &[])?;
        setter.type_parameter("Message", &[])?;
        setter.parameter(input).output(column()?);
        builder.item(item::Definition::method(
            format!("widget::table::Column::{name}"),
            receiver::Mode::Owned,
            setter.finish(),
        )?)?;
    }
    length_builders(builder, "widget::Table", &["width"])?;
    for name in ["padding", "padding_x", "padding_y", "separator", "separator_x", "separator_y"] {
        pixels_builders(builder, "widget::Table", name)?;
    }
    Ok(())
}

/// iced's `scrollable::Anchor` variants.
pub(crate) const SCROLLABLE_ANCHORS: [&str; 2] = ["Start", "End"];

/// iced's `scrollable::Source` variants.
pub(crate) const SCROLLABLE_SOURCES: [&str; 8] = [
    "Wheel",
    "Touch",
    "Scrollbar",
    "AutoScroll",
    "Operation",
    "Action",
    "Content",
    "Resize",
];

/// iced's `Degrees` and `Radians`: `Degrees(90.0)`, `Radians::PI`,
/// `Radians::from(degrees)` and their operators, the methods named for `std::ops`' traits.
fn declare_angles(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let f64 = || type_ref::Type::builtin(type_ref::Builtin::F64);
    let degrees = || type_ref::Type::registered("Degrees");
    let radians = || type_ref::Type::registered("Radians");
    let vector = || type_ref::Type::registered("Vector");
    let point = || type_ref::Type::registered("Point");
    for (path, output) in [("Degrees", degrees()?), ("Radians", radians()?)] {
        let mut from = signature::Builder::new();
        from.parameter(f64()).output(output.clone());
        builder.item(item::Definition::associated_function(
            format!("{path}::from"),
            from.finish(),
        )?)?;
    }
    let mut from = signature::Builder::new();
    from.parameter(degrees()?).output(radians()?);
    builder.item(item::Definition::associated_function("Radians::from", from.finish())?)?;
    // `f32::from(degrees)`: the angle as a number.
    for path in ["Degrees", "Radians"] {
        builder.item(item::Definition::method(
            format!("{path}::into"),
            receiver::Mode::Owned,
            output_signature_of(f64()),
        )?)?;
    }
    // `Rotation::Floating(angle)` and `Rotation::Solid(angle)`, matched as
    // in Rust, with the angle read in radians or degrees.
    let rotation = || type_ref::Type::registered("Rotation");
    for variant in ["Floating", "Solid"] {
        for input in [radians()?, degrees()?] {
            let mut constructor = signature::Builder::new();
            constructor.parameter(input).output(rotation()?);
            builder.item(item::Definition::associated_function(
                format!("Rotation::{variant}"),
                constructor.finish(),
            )?)?;
        }
        builder.item(item::Definition::method(
            format!("Rotation::{variant}"),
            receiver::Mode::Shared,
            output_signature_of(type_ref::Type::option(radians()?)),
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "Rotation::default",
        output_signature("Rotation")?,
    )?)?;
    for (name, output) in [("radians", "Radians"), ("degrees", "Degrees")] {
        builder.item(item::Definition::method(
            format!("Rotation::{name}"),
            receiver::Mode::Owned,
            output_signature(output)?,
        )?)?;
    }
    // `*rotation.radians_mut() = angle`: the rotation with its angle
    // replaced, which Rawr stores back into the place.
    let mut radians_mut = signature::Builder::new();
    radians_mut
        .parameter(rotation()?)
        .parameter(radians()?)
        .output(rotation()?);
    builder.item(item::Definition::associated_function(
        "Rotation::radians_mut",
        radians_mut.finish(),
    )?)?;
    for input in [rotation()?, radians()?, degrees()?] {
        builders(builder, "widget::Image", "rotation", [input])?;
    }
    builder.item(item::Definition::associated_value("Radians::PI", radians()?)?)?;
    for (owner, name, input, output) in [
        ("Degrees", "mul", f64(), degrees()?),
        ("Radians", "add", radians()?, radians()?),
        ("Radians", "add", degrees()?, radians()?),
        ("Radians", "sub", radians()?, radians()?),
        ("Radians", "mul", radians()?, radians()?),
        ("Radians", "mul", f64(), radians()?),
        ("Radians", "div", radians()?, radians()?),
        ("Radians", "div", f64(), radians()?),
        ("Radians", "rem", radians()?, radians()?),
        // iced's `Vector` arithmetic and a `Point` moved by a `Vector`.
        ("Vector", "add", vector()?, vector()?),
        ("Vector", "sub", vector()?, vector()?),
        ("Vector", "mul", f64(), vector()?),
        ("Vector", "div", f64(), vector()?),
        ("Point", "add", vector()?, point()?),
        ("Point", "sub", vector()?, point()?),
        ("Point", "sub", point()?, vector()?),
    ] {
        let mut operator = signature::Builder::new();
        operator.parameter(input).output(output);
        builder.item(item::Definition::method(
            format!("{owner}::{name}"),
            receiver::Mode::Owned,
            operator.finish(),
        )?)?;
    }
    Ok(())
}

/// iced's `text::Shaping` variants.
pub(crate) const TEXT_SHAPINGS: [&str; 3] = ["Auto", "Basic", "Advanced"];

/// The fields of iced's `checkbox::Icon` and their types.
pub(crate) const CHECKBOX_ICON_FIELDS: [(&str, &str); 5] = [
    ("font", "Font"),
    ("code_point", "char"),
    ("size", "Option<f64>"),
    ("line_height", "Option<widget::text::LineHeight>"),
    ("shaping", "widget::text::Shaping"),
];

/// `text::Shaping`, `text::LineHeight` and `checkbox::Icon`, whose struct
/// literal starts from `Icon::default()` and sets each field through its
/// builder. `Checkbox::icon` takes one.
fn declare_checkbox_icon(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for name in TEXT_SHAPINGS {
        builder.item(item::Definition::associated_value(
            format!("widget::text::Shaping::{name}"),
            type_ref::Type::registered("widget::text::Shaping")?,
        )?)?;
    }
    for name in ["Relative", "Absolute"] {
        for input in PIXELS {
            let mut variant = signature::Builder::new();
            variant.parameter(type_ref::Type::builtin(input));
            variant.output(type_ref::Type::registered("widget::text::LineHeight")?);
            builder.item(item::Definition::associated_function(
                format!("widget::text::LineHeight::{name}"),
                variant.finish(),
            )?)?;
        }
    }
    let icon = || type_ref::Type::registered("widget::checkbox::Icon");
    builder.item(item::Definition::associated_function(
        "widget::checkbox::Icon::default",
        output_signature("widget::checkbox::Icon")?,
    )?)?;
    for (field, ty) in CHECKBOX_ICON_FIELDS {
        let ty = match ty {
            "char" => type_ref::Type::builtin(type_ref::Builtin::Char),
            "Option<f64>" => type_ref::Type::option(type_ref::Type::builtin(type_ref::Builtin::F64)),
            "Option<widget::text::LineHeight>" => {
                type_ref::Type::option(type_ref::Type::registered("widget::text::LineHeight")?)
            }
            ty => type_ref::Type::registered(ty)?,
        };
        let mut reader = signature::Builder::new();
        reader.output(ty.clone());
        builder.item(item::Definition::method(
            format!("widget::checkbox::Icon::{field}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
        let mut setter = signature::Builder::new();
        setter.parameter(ty).output(icon()?);
        builder.item(item::Definition::method(
            format!("widget::checkbox::Icon::{field}"),
            receiver::Mode::Owned,
            setter.finish(),
        )?)?;
    }
    builders(builder, "widget::Checkbox", "icon", [icon()?])?;
    Ok(())
}

pub(crate) const FONT_WEIGHTS: [&str; 9] = [
    "Thin", "ExtraLight", "Light", "Normal", "Medium", "Semibold", "Bold", "ExtraBold", "Black",
];
pub(crate) const FONT_STRETCHES: [&str; 9] = [
    "UltraCondensed",
    "ExtraCondensed",
    "Condensed",
    "SemiCondensed",
    "Normal",
    "SemiExpanded",
    "Expanded",
    "ExtraExpanded",
    "UltraExpanded",
];
pub(crate) const FONT_STYLES: [&str; 3] = ["Normal", "Italic", "Oblique"];
pub(crate) const FONT_FAMILIES: [&str; 5] = ["Serif", "SansSerif", "Cursive", "Fantasy", "Monospace"];

/// iced's `tooltip::Position` variants.
pub(crate) const TOOLTIP_POSITIONS: [&str; 5] = ["Top", "Bottom", "Left", "Right", "FollowCursor"];

/// lilt's easings, which iced's `animation::Easing` is.
pub(crate) const EASINGS: [&str; 31] = [
    "Linear",
    "EaseIn",
    "EaseOut",
    "EaseInOut",
    "EaseInQuad",
    "EaseOutQuad",
    "EaseInOutQuad",
    "EaseInCubic",
    "EaseOutCubic",
    "EaseInOutCubic",
    "EaseInQuart",
    "EaseOutQuart",
    "EaseInOutQuart",
    "EaseInQuint",
    "EaseOutQuint",
    "EaseInOutQuint",
    "EaseInExpo",
    "EaseOutExpo",
    "EaseInOutExpo",
    "EaseInCirc",
    "EaseOutCirc",
    "EaseInOutCirc",
    "EaseInBack",
    "EaseOutBack",
    "EaseInOutBack",
    "EaseInElastic",
    "EaseOutElastic",
    "EaseInOutElastic",
    "EaseInBounce",
    "EaseOutBounce",
    "EaseInOutBounce",
];

/// `output<Message>` from `impl Fn(input) -> Message`, or from an
/// `Option` of one.
/// iced's `component::Component`, as script types implement it: its
/// `Theme` and `Renderer` are iced's, its `listen`, `mouse_interaction`,
/// `diff` and `operate` the defaults.
const COMPONENT: &str = "pub trait Component<Message> {
    type State: Default;
    type Event;
    fn update(&self, state: &mut Self::State, event: Self::Event, renderer: &iced::widget::Renderer) -> Option<Message>;
    fn view(&self, state: &Self::State) -> impl iced::widget::Widget<Self::Event>;
}";

/// A component's `Instance`: its value, `update` and `view`, and the
/// default of its State (`facade::Builder::implementable_trait`).
fn component_instance_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    for name in ["C", "S", "E", "Message"] {
        signature.type_parameter(name, &[])?;
    }
    let parameter = |name: &str| type_ref::Type::parameter(name);
    signature.parameter(parameter("C")?);
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([
            parameter("C")?,
            parameter("S")?,
            parameter("E")?,
            type_ref::Type::registered("widget::Renderer")?,
        ])?,
        type_ref::Type::option(parameter("Message")?),
    ));
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([parameter("C")?, parameter("S")?])?,
        type_ref::Type::applied("Element", [parameter("E")?])?,
    ));
    signature.parameter(parameter("S")?);
    signature.output(type_ref::Type::applied(
        "widget::component::Instance",
        [parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn component_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::applied(
        "widget::component::Instance",
        [type_ref::Type::parameter("Message")?],
    )?);
    signature.output(type_ref::Type::applied(
        "Element",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn message_mapper_signature(
    output: &str,
    input: type_ref::Type,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::callable(
        input,
        type_ref::Type::parameter("Message")?,
    ));
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn message_mapper_builder_signature(
    output: &str,
    input: type_ref::Builtin,
    optional: bool,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    let mapper = type_ref::Type::callable(
        type_ref::Type::builtin(input),
        type_ref::Type::parameter("Message")?,
    );
    signature.parameter(if optional {
        type_ref::Type::option(mapper)
    } else {
        mapper
    });
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn builtin_builder_signature(
    output: &str,
    input: type_ref::Builtin,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::builtin(input));
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

/// `owner<Message>` from `Message`, or from `Option<Message>`.
fn message_builder_signature(
    owner: &str,
    optional: bool,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    let message = type_ref::Type::parameter("Message")?;
    signature.parameter(if optional {
        type_ref::Type::option(message)
    } else {
        message
    });
    signature.output(type_ref::Type::applied(
        owner,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn column_align_x_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::registered("Alignment")?);
    signature.output(column_output()?);
    Ok(signature.finish())
}

fn registered_builder_signature(
    output: &str,
    parameter: &str,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::registered(parameter)?);
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn fixed_builder_signature(
    output: &str,
    input: type_ref::Type,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.parameter(input);
    signature.output(type_ref::Type::registered(output)?);
    Ok(signature.finish())
}

/// The pixel inputs of iced's `impl Into<Pixels>`.
const PIXELS: [type_ref::Builtin; 2] = [type_ref::Builtin::I64, type_ref::Builtin::F64];
const PADDING_SIDES: [&str; 7] = ["all", "top", "bottom", "left", "right", "horizontal", "vertical"];

/// One owned builder method of `owner` taking `input`.
fn builder_signature(
    owner: &str,
    input: type_ref::Type,
) -> Result<signature::Definition, host::Error> {
    if matches!(
        owner,
        "widget::Text"
            | "widget::Space"
            | "widget::ProgressBar"
            | "widget::Image"
            | "widget::Svg"
            | "widget::QRCode"
    ) {
        return fixed_builder_signature(owner, input);
    }
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(input);
    signature.output(type_ref::Type::applied(
        owner,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn builders(
    builder: &mut facade::Builder,
    owner: &str,
    name: &str,
    inputs: impl IntoIterator<Item = type_ref::Type>,
) -> Result<(), host::Error> {
    for input in inputs {
        builder.item(item::Definition::method(
            format!("{owner}::{name}"),
            receiver::Mode::Owned,
            builder_signature(owner, input)?,
        )?)?;
    }
    Ok(())
}

/// `impl Into<Pixels>`: an integer or float.
fn pixels_builders(
    builder: &mut facade::Builder,
    owner: &str,
    name: &str,
) -> Result<(), host::Error> {
    builders(builder, owner, name, PIXELS.map(type_ref::Type::builtin))
}

/// `impl Into<Length>`: a `Length`, or fixed pixels.
fn length_builders(
    builder: &mut facade::Builder,
    owner: &str,
    names: &[&str],
) -> Result<(), host::Error> {
    for name in names {
        builders(builder, owner, name, [type_ref::Type::registered("Length")?])?;
        pixels_builders(builder, owner, name)?;
    }
    Ok(())
}

/// `impl Into<Padding>`: a `Padding`, uniform pixels, or
/// `[vertical, horizontal]` pixels, as iced's `From` impls.
fn padding_builders(builder: &mut facade::Builder, owner: &str) -> Result<(), host::Error> {
    builders(builder, owner, "padding", [type_ref::Type::registered("Padding")?])?;
    pixels_builders(builder, owner, "padding")?;
    builders(
        builder,
        owner,
        "padding",
        PIXELS.map(|pixels| type_ref::Type::array(type_ref::Type::builtin(pixels), 2)),
    )
}

/// iced's `pick_list(selected: Option<T>, options: &[T], to_string: impl
/// Fn(&T) -> String)`.
fn pick_list_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("T", &[])?;
    signature.type_parameter("Message", &[])?;
    let option = type_ref::Type::parameter("T")?;
    signature.parameter(type_ref::Type::option(option.clone()));
    signature.parameter(type_ref::Type::slice(option.clone()));
    signature.parameter(type_ref::Type::callable(
        option,
        type_ref::Type::builtin(type_ref::Builtin::String),
    ));
    signature.output(pick_list_output()?);
    Ok(signature.finish())
}

fn pick_list_output() -> Result<type_ref::Type, host::Error> {
    type_ref::Type::applied(
        "widget::PickList",
        [
            type_ref::Type::parameter("T")?,
            type_ref::Type::parameter("Message")?,
        ],
    )
}

/// `PickList::on_select(impl Fn(T) -> Message)`, `placeholder`, `width`,
/// `padding` and `text_size`.
fn pick_list_builders(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let mut inputs = vec![
        (
            "on_select",
            type_ref::Type::callable(
                type_ref::Type::parameter("T")?,
                type_ref::Type::parameter("Message")?,
            ),
        ),
        (
            "placeholder",
            type_ref::Type::builtin(type_ref::Builtin::String),
        ),
        ("width", type_ref::Type::registered("Length")?),
        ("padding", type_ref::Type::registered("Padding")?),
    ];
    for name in ["width", "padding", "text_size"] {
        inputs.extend(PIXELS.map(|pixels| (name, type_ref::Type::builtin(pixels))));
    }
    for (name, input) in inputs {
        let mut signature = signature::Builder::new();
        signature.type_parameter("T", &[])?;
        signature.type_parameter("Message", &[])?;
        signature.parameter(input);
        signature.output(pick_list_output()?);
        builder.item(item::Definition::method(
            format!("widget::PickList::{name}"),
            receiver::Mode::Owned,
            signature.finish(),
        )?)?;
    }
    Ok(())
}

/// iced's `combo_box(state: &combo_box::State<T>, placeholder: &str,
/// selection: Option<&T>, on_selected: impl Fn(T) -> Message)`, its
/// `State::new(options: Vec<T>)` and the builders of the `ComboBox`. iced
/// labels the options by `T: Display`: `State::new` declares that bound as
/// its last input, `impl Fn(T) -> String`, which the compiler fills with
/// `T::to_string`.
fn declare_combo_box(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let t = || type_ref::Type::parameter("T");
    let state = || type_ref::Type::applied("widget::combo_box::State", [t()?]);
    let output = || {
        type_ref::Type::applied(
            "widget::ComboBox",
            [t()?, type_ref::Type::parameter("Message")?],
        )
    };
    let mut signature = signature::Builder::new();
    signature.type_parameter("T", &[])?;
    signature.parameter(type_ref::Type::slice(t()?));
    signature.parameter(type_ref::Type::callable(
        t()?,
        type_ref::Type::builtin(type_ref::Builtin::String),
    ));
    signature.output(state()?);
    builder.item(item::Definition::associated_function(
        "widget::combo_box::State::new",
        signature.finish(),
    )?)?;

    let mut signature = signature::Builder::new();
    signature.type_parameter("T", &[])?;
    signature.type_parameter("Message", &[])?;
    signature.parameter(state()?);
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    signature.parameter(type_ref::Type::option(t()?));
    signature.parameter(type_ref::Type::callable(t()?, type_ref::Type::parameter("Message")?));
    signature.output(output()?);
    builder.item(item::Definition::function("widget::combo_box", signature.finish())?)?;

    let mut inputs = vec![
        (
            "on_option_hovered",
            type_ref::Type::callable(t()?, type_ref::Type::parameter("Message")?),
        ),
        ("on_open", type_ref::Type::parameter("Message")?),
        ("on_close", type_ref::Type::parameter("Message")?),
        ("width", type_ref::Type::registered("Length")?),
    ];
    for name in ["width", "size"] {
        inputs.extend(PIXELS.map(|pixels| (name, type_ref::Type::builtin(pixels))));
    }
    for (name, input) in inputs {
        let mut signature = signature::Builder::new();
        signature.type_parameter("T", &[])?;
        signature.type_parameter("Message", &[])?;
        signature.parameter(input);
        signature.output(output()?);
        builder.item(item::Definition::method(
            format!("widget::ComboBox::{name}"),
            receiver::Mode::Owned,
            signature.finish(),
        )?)?;
    }
    Ok(())
}

/// iced's `radio(label: impl Into<String>, value: V, selected: Option<V>,
/// on_click: impl FnOnce(V) -> Message)` with `V: Copy + Eq`.
fn radio_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter(
        "V",
        &[
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
        ],
    )?;
    signature.type_parameter("Message", &[])?;
    let value = type_ref::Type::parameter("V")?;
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    signature.parameter(value.clone());
    signature.parameter(type_ref::Type::option(value.clone()));
    signature.parameter(type_ref::Type::callable(
        value,
        type_ref::Type::parameter("Message")?,
    ));
    signature.output(type_ref::Type::applied(
        "widget::Radio",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

/// `step`, `default` and `shift_step` take the slider's own number.
fn slider_step_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("T", &[])?;
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::parameter("T")?);
    signature.output(slider_output()?);
    Ok(signature.finish())
}

/// A `Slider<T, Message>` builder taking `input`.
fn slider_input_signature(input: type_ref::Type) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("T", &[])?;
    signature.type_parameter("Message", &[])?;
    signature.parameter(input);
    signature.output(slider_output()?);
    Ok(signature.finish())
}

/// `Slider<T, Message>`; its signature declares `T` and `Message`.
fn slider_output() -> Result<type_ref::Type, host::Error> {
    type_ref::Type::applied(
        "widget::Slider",
        [
            type_ref::Type::parameter("T")?,
            type_ref::Type::parameter("Message")?,
        ],
    )
}

fn column_output() -> Result<type_ref::Type, host::Error> {
    type_ref::Type::applied("widget::Column", [type_ref::Type::parameter("Message")?])
}

fn element_map_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.type_parameter("Parent", &[])?;
    signature.parameter(type_ref::Type::callable(
        type_ref::Type::parameter("Message")?,
        type_ref::Type::parameter("Parent")?,
    ));
    signature.output(type_ref::Type::applied(
        "Element",
        [type_ref::Type::parameter("Parent")?],
    )?);
    Ok(signature.finish())
}

fn element_boxed_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.output(type_ref::Type::applied(
        "Element",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

/// `Theme::palette()` and `Theme::seed()`, with the palette's public fields.
fn declare_palette(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for (name, output) in [("palette", "theme::Palette"), ("seed", "theme::palette::Seed")] {
        let mut method = signature::Builder::new();
        method.output(type_ref::Type::registered(output)?);
        builder.item(item::Definition::method(
            format!("Theme::{name}"),
            receiver::Mode::Owned,
            method.finish(),
        )?)?;
    }
    // `Theme::custom(name, Seed { background, .. })`. A facade struct
    // literal starts from `Seed::default()` (iced's light seed) and sets
    // each field through its one-argument builder.
    let seed = || type_ref::Type::registered("theme::palette::Seed");
    let mut default = signature::Builder::new();
    default.output(seed()?);
    builder.item(item::Definition::associated_function(
        "theme::palette::Seed::default",
        default.finish(),
    )?)?;
    for &(name, _) in crate::theme::SEEDS {
        builder.item(item::Definition::associated_value(
            format!("theme::palette::Seed::{name}"),
            seed()?,
        )?)?;
    }
    for &(owner, field, _) in PALETTE_FIELDS {
        if owner != "theme::palette::Seed" {
            continue;
        }
        let mut setter = signature::Builder::new();
        setter
            .parameter(type_ref::Type::registered("Color")?)
            .output(seed()?);
        builder.item(item::Definition::method(
            format!("{owner}::{field}"),
            receiver::Mode::Owned,
            setter.finish(),
        )?)?;
    }
    let mut custom = signature::Builder::new();
    custom
        .parameter(type_ref::Type::builtin(type_ref::Builtin::String))
        .parameter(seed()?)
        .output(type_ref::Type::registered("Theme")?);
    builder.item(item::Definition::associated_function(
        "Theme::custom",
        custom.finish(),
    )?)?;
    for &(owner, field, output) in PALETTE_FIELDS {
        let mut reader = signature::Builder::new();
        reader.output(match output {
            "bool" => type_ref::Type::builtin(type_ref::Builtin::Bool),
            output => type_ref::Type::registered(output)?,
        });
        // A parameterless shared reader is read as a field: `palette.primary`.
        builder.item(item::Definition::method(
            format!("{owner}::{field}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
    }
    Ok(())
}

/// The public fields of iced's palette types: owner, field, field type.
const PALETTE_FIELDS: &[(&str, &str, &str)] = &[
    ("theme::Palette", "background", "theme::palette::Background"),
    ("theme::Palette", "primary", "theme::palette::Swatch"),
    ("theme::Palette", "secondary", "theme::palette::Swatch"),
    ("theme::Palette", "success", "theme::palette::Swatch"),
    ("theme::Palette", "warning", "theme::palette::Swatch"),
    ("theme::Palette", "danger", "theme::palette::Swatch"),
    ("theme::Palette", "is_dark", "bool"),
    ("theme::palette::Background", "base", "theme::palette::Pair"),
    ("theme::palette::Background", "weakest", "theme::palette::Pair"),
    ("theme::palette::Background", "weaker", "theme::palette::Pair"),
    ("theme::palette::Background", "weak", "theme::palette::Pair"),
    ("theme::palette::Background", "neutral", "theme::palette::Pair"),
    ("theme::palette::Background", "strong", "theme::palette::Pair"),
    ("theme::palette::Background", "stronger", "theme::palette::Pair"),
    ("theme::palette::Background", "strongest", "theme::palette::Pair"),
    ("theme::palette::Swatch", "base", "theme::palette::Pair"),
    ("theme::palette::Swatch", "weak", "theme::palette::Pair"),
    ("theme::palette::Swatch", "strong", "theme::palette::Pair"),
    ("theme::palette::Pair", "color", "Color"),
    ("theme::palette::Pair", "text", "Color"),
    ("theme::palette::Seed", "background", "Color"),
    ("theme::palette::Seed", "text", "Color"),
    ("theme::palette::Seed", "primary", "Color"),
    ("theme::palette::Seed", "success", "Color"),
    ("theme::palette::Seed", "warning", "Color"),
    ("theme::palette::Seed", "danger", "Color"),
];

/// `container::Style` and `Border` with their pinned iced builders.
fn declare_styles(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let style = || type_ref::Type::registered("widget::container::Style");
    let border = || type_ref::Type::registered("Border");
    let color = || type_ref::Type::registered("Color");
    let pixels = PIXELS.map(type_ref::Type::builtin);
    for (owner, output) in [("widget::container::Style", style()?), ("Border", border()?)] {
        let mut default = signature::Builder::new();
        default.output(output);
        builder.item(item::Definition::associated_function(
            format!("{owner}::default"),
            default.finish(),
        )?)?;
    }
    // `background` is a field (`widget::style`): its builder takes a color
    // or a background, and a literal takes an `Option`.
    for (name, input) in [("border", border()?), ("color", color()?)] {
        let mut method = signature::Builder::new();
        method.parameter(input).output(style()?);
        builder.item(item::Definition::method(
            format!("widget::container::Style::{name}"),
            receiver::Mode::Owned,
            method.finish(),
        )?)?;
    }
    // `style.border`, read as a field.
    let mut reader = signature::Builder::new();
    reader.output(border()?);
    builder.item(item::Definition::method(
        "widget::container::Style::border",
        receiver::Mode::Shared,
        reader.finish(),
    )?)?;
    // `border.width` and `border.color`, read as fields, and
    // `border::radius(r)`.
    for (name, output) in [("width", type_ref::Type::builtin(type_ref::Builtin::F64)), ("color", color()?)] {
        let mut reader = signature::Builder::new();
        reader.output(output);
        builder.item(item::Definition::method(
            format!("Border::{name}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
    }
    for input in &pixels {
        let mut function = signature::Builder::new();
        function
            .parameter(input.clone())
            .output(type_ref::Type::registered("border::Radius")?);
        builder.item(item::Definition::function("border::radius", function.finish())?)?;
    }
    let border_inputs = [
        ("rounded", &pixels[..]),
        ("width", &pixels[..]),
        ("color", &[color()?][..]),
    ];
    for (name, inputs) in border_inputs {
        for input in inputs {
            let mut function = signature::Builder::new();
            function.parameter(input.clone()).output(border()?);
            builder.item(item::Definition::function(
                format!("border::{name}"),
                function.finish(),
            )?)?;
            let mut method = signature::Builder::new();
            method.parameter(input.clone()).output(border()?);
            builder.item(item::Definition::method(
                format!("Border::{name}"),
                receiver::Mode::Owned,
                method.finish(),
            )?)?;
        }
    }
    Ok(())
}

fn declare_canonical_values(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for name in ["BLACK", "TRANSPARENT", "WHITE"] {
        builder.item(item::Definition::associated_value(
            format!("Color::{name}"),
            type_ref::Type::registered("Color")?,
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "Color::from_rgb",
        repeated_builtin_signature(type_ref::Builtin::F64, 3, "Color")?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "Color::from_rgba",
        repeated_builtin_signature(type_ref::Builtin::F64, 4, "Color")?,
    )?)?;
    // `u8` channels and `u32` packed hex (`0xrrggbb`), as integers.
    for (name, bytes, alpha) in [
        ("from_rgb8", 3, false),
        ("from_rgba8", 3, true),
        ("from_packed_rgb8", 1, false),
        ("from_packed_rgba8", 1, true),
    ] {
        let mut signature = signature::Builder::new();
        for _ in 0..bytes {
            signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
        }
        if alpha {
            signature.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
        }
        signature.output(type_ref::Type::registered("Color")?);
        builder.item(item::Definition::associated_function(
            format!("Color::{name}"),
            signature.finish(),
        )?)?;
    }

    for name in ["Fit", "Fill", "Shrink"] {
        builder.item(item::Definition::associated_value(
            format!("Length::{name}"),
            type_ref::Type::registered("Length")?,
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "Length::Fixed",
        integer_signature("Length", 1)?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "Length::Fixed",
        repeated_builtin_signature(type_ref::Builtin::F64, 1, "Length")?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "Length::FillPortion",
        integer_signature("Length", 1)?,
    )?)?;
    // iced's `impl From<f32> for Length` and its kin: a fixed length, as
    // `impl Into<Length>` takes a number.
    for input in PIXELS {
        let mut from = signature::Builder::new();
        from.parameter(type_ref::Type::builtin(input));
        from.output(type_ref::Type::registered("Length")?);
        builder.item(item::Definition::associated_function("Length::from", from.finish())?)?;
    }
    // A `T: Into<Length>` as the `Length` it converts to: the value, a
    // `Length` or a number, settled when it runs.
    let mut into = signature::Builder::new();
    into.type_parameter("T", &[])?;
    into.parameter(type_ref::Type::parameter("T")?);
    into.output(type_ref::Type::registered("Length")?);
    builder.item(item::Definition::associated_function("Length::__into", into.finish())?)?;
    // `Fit.max(600)`: a length with a bound, as iced's `Length::min` and
    // `Length::max`.
    for name in ["min", "max"] {
        for input in PIXELS {
            let mut bound = signature::Builder::new();
            bound.parameter(type_ref::Type::builtin(input));
            bound.output(type_ref::Type::registered("Length")?);
            builder.item(item::Definition::method(
                format!("Length::{name}"),
                receiver::Mode::Owned,
                bound.finish(),
            )?)?;
        }
    }
    builder.item(item::Definition::associated_value(
        "Padding::ZERO",
        type_ref::Type::registered("Padding")?,
    )?)?;
    for input in PIXELS {
        builder.item(item::Definition::associated_function(
            "Padding::new",
            repeated_builtin_signature(input, 1, "Padding")?,
        )?)?;
        for side in PADDING_SIDES {
            builder.item(item::Definition::function(
                format!("padding::{side}"),
                repeated_builtin_signature(input, 1, "Padding")?,
            )?)?;
            if side != "all" {
                builder.item(item::Definition::method(
                    format!("Padding::{side}"),
                    receiver::Mode::Owned,
                    repeated_builtin_signature(input, 1, "Padding")?,
                )?)?;
            }
        }
    }
    for name in ["DEFAULT", "MONOSPACE"] {
        builder.item(item::Definition::associated_value(
            format!("Font::{name}"),
            type_ref::Type::registered("Font")?,
        )?)?;
    }
    builder.item(item::Definition::associated_function(
        "Point::new",
        repeated_builtin_signature(type_ref::Builtin::F64, 2, "Point")?,
    )?)?;
    builder.item(item::Definition::associated_function(
        "Vector::new",
        repeated_builtin_signature(type_ref::Builtin::F64, 2, "Vector")?,
    )?)?;

    let mut rectangle = signature::Builder::new();
    rectangle.parameter(type_ref::Type::registered("Point")?);
    rectangle.parameter(type_ref::Type::registered("Size")?);
    rectangle.output(type_ref::Type::registered("Rectangle")?);
    builder.item(item::Definition::associated_function(
        "Rectangle::new",
        rectangle.finish(),
    )?)?;

    let mut size = signature::Builder::new();
    size.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
    size.parameter(type_ref::Type::builtin(type_ref::Builtin::F64));
    size.output(type_ref::Type::registered("Size")?);
    builder.item(item::Definition::associated_function(
        "Size::new",
        size.finish(),
    )?)?;
    for name in INTERACTIONS {
        builder.item(item::Definition::associated_value(
            format!("mouse::Interaction::{name}"),
            type_ref::Type::registered("mouse::Interaction")?,
        )?)?;
    }
    Ok(())
}

const INTERACTIONS: &[&str] = &[
    "None",
    "Hidden",
    "Idle",
    "ContextMenu",
    "Help",
    "Pointer",
    "Progress",
    "Wait",
    "Cell",
    "Crosshair",
    "Text",
    "Alias",
    "Copy",
    "Move",
    "NoDrop",
    "NotAllowed",
    "Grab",
    "Grabbing",
    "ResizingHorizontally",
    "ResizingVertically",
    "ResizingDiagonallyUp",
    "ResizingDiagonallyDown",
    "ResizingColumn",
    "ResizingRow",
    "AllScroll",
    "ZoomIn",
    "ZoomOut",
];

fn repeated_builtin_signature(
    input: type_ref::Builtin,
    count: usize,
    output: &str,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    for _ in 0..count {
        signature.parameter(type_ref::Type::builtin(input));
    }
    signature.output(type_ref::Type::registered(output)?);
    Ok(signature.finish())
}

fn output_signature_of(output: type_ref::Type) -> signature::Definition {
    let mut signature = signature::Builder::new();
    signature.output(output);
    signature.finish()
}

fn output_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.output(type_ref::Type::registered(output)?);
    Ok(signature.finish())
}

fn integer_signature(output: &str, count: usize) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    for _ in 0..count {
        signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    }
    signature.output(type_ref::Type::registered(output)?);
    Ok(signature.finish())
}

fn bool_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.output(type_ref::Type::builtin(type_ref::Builtin::Bool));
    Ok(signature.finish())
}

fn message_output_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn message_transform_signature(output: &str) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::parameter("Message")?);
    signature.output(type_ref::Type::applied(
        output,
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn layout_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.parameter(type_ref::Type::registered("widget::layout::Size")?);
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::I64));
    signature.output(type_ref::Type::registered("widget::layout::Node")?);
    Ok(signature.finish())
}

fn event_outcome_when_signature() -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    signature.parameter(type_ref::Type::builtin(type_ref::Builtin::Bool));
    signature.output(type_ref::Type::applied(
        "widget::event::Outcome",
        [type_ref::Type::parameter("Message")?],
    )?);
    Ok(signature.finish())
}

fn string_constructor_signature(
    output: &str,
    count: usize,
) -> Result<signature::Definition, host::Error> {
    let mut signature = signature::Builder::new();
    for _ in 0..count {
        signature.parameter(type_ref::Type::builtin(type_ref::Builtin::String));
    }
    signature.output(type_ref::Type::registered(output)?);
    Ok(signature.finish())
}

const BUILT_IN_TEXT_STYLES: [&str; 7] =
    ["default", "base", "primary", "secondary", "success", "warning", "danger"];
const BUILT_IN_CONTAINER_STYLES: [&str; 9] = [
    "transparent",
    "rounded_box",
    "bordered_box",
    "dark",
    "primary",
    "secondary",
    "success",
    "warning",
    "danger",
];

const BUILT_IN_THEMES: [&str; 22] = [
    "Light",
    "Dark",
    "Dracula",
    "Nord",
    "SolarizedLight",
    "SolarizedDark",
    "GruvboxLight",
    "GruvboxDark",
    "CatppuccinLatte",
    "CatppuccinFrappe",
    "CatppuccinMacchiato",
    "CatppuccinMocha",
    "TokyoNight",
    "TokyoNightStorm",
    "TokyoNightLight",
    "KanagawaWave",
    "KanagawaDragon",
    "KanagawaLotus",
    "Moonfly",
    "Nightfly",
    "Oxocarbon",
    "Ferra",
];

fn state_message_parameters(signature: &mut signature::Builder) -> Result<(), host::Error> {
    signature.type_parameter("State", &[])?;
    signature.type_parameter(
        "Message",
        &[
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use iced::advanced::widget::{tree, Tree};
    use rawr::timing::{Outcome, Phase};
    use rawr::{host, Dynamic, Engine, EvalAltResult, Position, Shared};

    use super::marker;

    #[test]
    fn exact_runtime_item_binding_executes_only_through_bound_launch() {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let candidate = engine.compile_application(text_application()).unwrap();
        let descriptor = candidate.validate().unwrap();
        let launch = engine.launch_application(descriptor).unwrap();

        let mut facade = host::runtime::Builder::new(super::facade().unwrap());
        bind_text_projection(&mut facade);
        facade
            .bind_with_context("iced::widget::text", |call, _| {
                Ok(Dynamic::from(test_text(call)))
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(facade.finish()).unwrap();
        let bound = launch.bind(registry).unwrap();

        let mut state = bound.boot_default().unwrap();
        let output = bound
            .invoke(
                bound.view().callback().item(),
                std::slice::from_mut(&mut state),
            )
            .unwrap();
        assert!(output.is::<marker::Text>());
    }

    #[test]
    fn payload_free_widget_carriers_retain_their_exact_message_witness() {
        for source in [
            text_application(),
            button_application(),
            empty_column_application(),
        ] {
            let mut engine = Engine::new();
            super::install(&mut engine).unwrap();
            let engine = Shared::new(engine);
            let descriptor = engine.compile_application_descriptor(source).unwrap();
            let launch = engine.launch_application(descriptor).unwrap();
            let mut registry = host::runtime::Registry::new();
            super::install_test_runtime(&mut registry);

            let adapter = crate::program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();
            assert!(adapter.last_good_output().is_some());
        }
    }

    #[test]
    fn positional_column_keeps_native_tree_while_keyed_column_owns_reconciliation() {
        // iced master's own column keeps a layout cache in its tree.
        use iced::widget::Widget as _;
        let native: iced::Element<'static, crate::runtime::Message> =
            iced::widget::column(Vec::<iced::Element<'static, crate::runtime::Message>>::new())
                .boxed();
        let native = Tree::new(&native).tag;

        let positional = boot(column_application());
        let context = positional.context();
        let positional = positional.display().lower(&context);
        let positional = Tree::new(&positional);
        assert_eq!(positional.tag, native);

        let keyed = boot(keyed_column_application());
        let context = keyed.context();
        let keyed = keyed.display().lower(&context);
        let keyed = Tree::new(&keyed);
        assert_ne!(keyed.tag, native);
    }

    #[test]
    fn keyed_column_invalid_views_produce_one_positioned_fallback_and_recover() {
        for (duplicate, expected) in [(false, "require a key"), (true, "repeat key")] {
            let adapter = boot_observed(&invalid_keyed_column_application(duplicate));
            assert!(adapter.last_good_output().is_none());
            let [entry] = adapter.diagnostics() else {
                panic!("one invalid keyed Column view report must be displayed");
            };
            assert!(entry.message().contains(expected), "{}", entry.message());
            let [label] = entry.report().labels() else {
                panic!("the invalid Column call must retain exact source evidence");
            };
            assert_eq!(label.source().identity(), "<memory>");
            assert!(label.span().start().line().is_some());
        }

        let recovered = boot(keyed_column_application());
        assert!(recovered.last_good_output().is_some());
        assert!(recovered.diagnostics().is_empty());
    }

    fn boot(source: &str) -> crate::program::Adapter {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        super::install_test_runtime(&mut registry);
        crate::program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
    }

    fn boot_observed(source: &str) -> crate::program::Adapter {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        super::install_test_runtime(&mut registry);
        crate::program::Adapter::boot_observed(launch.bind(registry).unwrap()).unwrap()
    }

    #[test]
    fn runtime_binding_output_must_match_the_proven_instantiation() {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(text_application())
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();

        let mut facade = host::runtime::Builder::new(super::facade().unwrap());
        bind_text_projection(&mut facade);
        facade
            .bind("iced::widget::text", |_| Ok(Dynamic::from(42_i64)))
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(facade.finish()).unwrap();
        let bound = launch.bind(registry).unwrap();
        let mut state = bound.boot_default().unwrap();
        let error = bound
            .invoke(
                bound.view().callback().item(),
                std::slice::from_mut(&mut state),
            )
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("returned `i64`, expected `iced::widget::Text`"),
            "{error}"
        );
    }

    #[test]
    fn view_failure_keeps_one_last_good_overlay_until_a_successful_recovery() {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let candidate = engine
            .compile_application(failing_text_application())
            .unwrap();
        let tick = candidate
            .items()
            .find(|item| item.key().name() == "Tick")
            .unwrap()
            .clone();
        let descriptor = candidate.validate().unwrap();
        let launch = engine.launch_application(descriptor).unwrap();

        let view_evaluations = Arc::new(AtomicUsize::new(0));
        let mut facade = host::runtime::Builder::new(super::facade().unwrap());
        bind_widget_attestations(&mut facade);
        let evaluations = view_evaluations.clone();
        facade
            .bind_with_context("iced::widget::text", move |call, args| {
                evaluations.fetch_add(1, Ordering::Relaxed);
                let content = args[0].as_int().unwrap().to_string();
                Ok(Dynamic::from(marker::Text {
                    witness: call.output_witness().unwrap(),
                    identity: crate::widget::Identity::unique(),
                    content,
                    size: None,
                    width: iced::Length::Fit,
                    height: None,
                    align_x: iced::advanced::text::Alignment::Default,
                    align_y: None,
                    style: None,
                    color: None,
                    font: None,
                    line_height: None,
                }))
            })
            .unwrap();
        facade
            .bind("iced::widget::Text::size", |args| {
                let mut text = args[0].take().try_cast_result::<marker::Text>().unwrap();
                if text.content == "1" {
                    Err(EvalAltResult::ErrorRuntime(
                        "deliberate view failure".into(),
                        Position::NONE,
                    )
                    .into())
                } else {
                    text.size = Some(args[1].as_int().unwrap() as f32);
                    Ok(Dynamic::from(text))
                }
            })
            .unwrap();
        facade
            .bind_projection_with_context("iced::widget::Text", "iced::Element", |call, args| {
                let text = args[0].take().try_cast_result::<marker::Text>().unwrap();
                Ok(Dynamic::from(
                    crate::widget::Value::from_projection(text.clone(), call, &text.witness)
                        .unwrap(),
                ))
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(facade.finish()).unwrap();
        let bound = launch.bind(registry).unwrap();
        let payload = bound.invoke(&tick, &mut []).unwrap();
        let mut adapter = crate::program::Adapter::boot_observed(bound).unwrap();

        let instance = adapter.instance();
        let generation = adapter.generation();
        let last_good = adapter.last_good_output().unwrap().identity();
        assert_eq!(view_evaluations.load(Ordering::Relaxed), 1);
        assert_view(&adapter, "0", false);

        let message = adapter.context().message(payload.clone());
        assert_eq!(
            adapter.update(message).unwrap(),
            crate::program::Delivery::Applied
        );
        assert_eq!(adapter.instance(), instance);
        assert_eq!(adapter.generation(), generation);
        assert_eq!(adapter.last_good_output().unwrap().identity(), last_good);
        assert_eq!(adapter.display().identity(), last_good);
        assert_eq!(adapter.diagnostics().len(), 1);
        let entry = &adapter.diagnostics()[0];
        let message = entry.message().to_owned();
        assert!(message.contains("deliberate view failure"), "{message}");
        assert_eq!(entry.report().code(), rawr::diagnostic::code::RUNTIME_THROW);
        assert_eq!(view_evaluations.load(Ordering::Relaxed), 2);
        assert_eq!(
            adapter
                .timing()
                .attempts()
                .iter()
                .filter(|attempt| attempt.phase() == Phase::ViewEvaluation)
                .map(|attempt| attempt.outcome())
                .collect::<Vec<_>>(),
            vec![Outcome::Success, Outcome::Failure]
        );

        for _ in 0..2 {
            assert_view(&adapter, "0", true);
            let mut simulator = iced_test::simulator(adapter.view());
            assert!(simulator.find(message.as_str()).is_ok());
            assert!(simulator
                .find("view · runtime · error[RAWR-RUNTIME-THROW]")
                .is_ok());
        }
        assert_eq!(adapter.diagnostics().len(), 1);
        assert_eq!(view_evaluations.load(Ordering::Relaxed), 2);

        let message = adapter.context().message(payload);
        assert_eq!(
            adapter.update(message).unwrap(),
            crate::program::Delivery::Applied
        );
        assert_eq!(adapter.instance(), instance);
        assert_eq!(adapter.generation(), generation);
        assert!(adapter.diagnostics().is_empty());
        assert_ne!(adapter.last_good_output().unwrap().identity(), last_good);
        assert_eq!(view_evaluations.load(Ordering::Relaxed), 3);
        assert_eq!(
            adapter
                .timing()
                .attempts()
                .iter()
                .filter(|attempt| attempt.phase() == Phase::ViewEvaluation)
                .map(|attempt| attempt.outcome())
                .collect::<Vec<_>>(),
            vec![Outcome::Success, Outcome::Failure, Outcome::Success]
        );
        assert_view(&adapter, "2", false);
    }

    fn assert_view(adapter: &crate::program::Adapter, content: &str, has_diagnostic: bool) {
        let mut simulator = iced_test::simulator(adapter.view());
        assert!(simulator.find(content).is_ok());
        assert_eq!(
            simulator.find(crate::diagnostic::OVERLAY_TITLE).is_ok(),
            has_diagnostic
        );
    }

    #[test]
    fn expression_and_boundary_projections_share_one_semantic_binding() {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(column_application())
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();

        let item_projections = Arc::new(AtomicUsize::new(0));
        let boundary_projections = Arc::new(AtomicUsize::new(0));
        let mut facade = host::runtime::Builder::new(super::facade().unwrap());
        bind_widget_attestations(&mut facade);
        facade
            .bind_with_context("iced::widget::text", |call, _| {
                Ok(Dynamic::from(test_text(call)))
            })
            .unwrap();
        facade
            .bind_macro_with_context("iced::widget::column", |call, args| {
                assert_eq!(args.len(), 2);
                assert!(args.iter().all(|value| value.is::<crate::widget::Value>()));
                Ok(Dynamic::from(test_column(call)))
            })
            .unwrap();
        let count = item_projections.clone();
        facade
            .bind_projection_with_context(
                "iced::widget::Text",
                "iced::Element",
                move |call, args| {
                    count.fetch_add(1, Ordering::Relaxed);
                    let text = args[0].read_lock::<marker::Text>().unwrap();
                    Ok(Dynamic::from(
                        crate::widget::diagnostic_from_projection(
                            "projected child",
                            call,
                            &text.witness,
                        )
                        .unwrap(),
                    ))
                },
            )
            .unwrap();
        let count = boundary_projections.clone();
        facade
            .bind_projection_with_context(
                "iced::widget::Column",
                "iced::Element",
                move |call, args| {
                    count.fetch_add(1, Ordering::Relaxed);
                    let column = args[0].read_lock::<marker::Column>().unwrap();
                    Ok(Dynamic::from(
                        crate::widget::diagnostic_from_projection(
                            "projected root",
                            call,
                            &column.witness,
                        )
                        .unwrap(),
                    ))
                },
            )
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(facade.finish()).unwrap();

        let adapter = crate::program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();
        assert!(adapter.last_good_output().is_some());
        assert_eq!(item_projections.load(Ordering::Relaxed), 2);
        assert_eq!(boundary_projections.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn launch_requires_projection_bindings_before_callbacks_can_run() {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(text_application())
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();

        let mut facade = host::runtime::Builder::new(super::facade().unwrap());
        bind_widget_attestations(&mut facade);
        facade
            .bind_with_context("iced::widget::text", |call, _| {
                Ok(Dynamic::from(test_text(call)))
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(facade.finish()).unwrap();

        let error = launch.bind(registry).unwrap_err();
        assert!(matches!(
            error,
            host::runtime::Error::MissingProjectionBinding { ref projection }
                if projection.contains("iced::widget::Text -> iced::Element")
        ));
    }

    #[test]
    fn projection_results_are_checked_at_the_original_source_site() {
        let mut engine = Engine::new();
        super::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(text_application())
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();

        let mut facade = host::runtime::Builder::new(super::facade().unwrap());
        bind_widget_attestations(&mut facade);
        facade
            .bind_with_context("iced::widget::text", |call, _| {
                Ok(Dynamic::from(test_text(call)))
            })
            .unwrap();
        facade
            .bind_projection("iced::widget::Text", "iced::Element", |_| {
                Ok(Dynamic::from(42_i64))
            })
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(facade.finish()).unwrap();
        let bound = launch.bind(registry).unwrap();
        let mut state = bound.boot_default().unwrap();

        let error = bound
            .invoke_view(std::slice::from_mut(&mut state))
            .unwrap_err();
        let rawr::program::Error::Evaluation { error, .. } = error else {
            panic!("expected positioned projection evaluation error");
        };
        assert!(
            error.to_string().contains(
                "runtime projection `iced::widget::Text -> iced::Element` returned `i64`"
            ),
            "{error}"
        );
        assert!(error.position().line().is_some());
    }

    fn bind_text_projection(builder: &mut host::runtime::Builder) {
        bind_widget_attestations(builder);
        builder
            .bind_projection_with_context("iced::widget::Text", "iced::Element", |call, args| {
                let text = args[0].read_lock::<marker::Text>().unwrap();
                Ok(Dynamic::from(
                    crate::widget::diagnostic_from_projection(
                        "projected text",
                        call,
                        &text.witness,
                    )
                    .unwrap(),
                ))
            })
            .unwrap();
    }

    fn bind_widget_attestations(builder: &mut host::runtime::Builder) {
        builder
            .bind_type_attestation("iced::Element", |value, expected| {
                let value = value.read_lock::<crate::widget::Value>()?;
                expected.attest_witness(value.witness())
            })
            .unwrap();
        builder
            .bind_type_attestation("iced::widget::Column", |value, expected| {
                let value = value.read_lock::<marker::Column>()?;
                expected.attest_witness(Some(&value.witness))
            })
            .unwrap();
    }

    fn test_text(call: &host::runtime::Call<'_>) -> marker::Text {
        marker::Text {
            witness: call.output_witness().unwrap(),
            identity: crate::widget::Identity::unique(),
            content: "test text".to_owned(),
            size: None,
            width: iced::Length::Fit,
            height: None,
            align_x: iced::advanced::text::Alignment::Default,
            align_y: None,
            style: None,
            color: None,
            font: None,
            line_height: None,
        }
    }

    fn test_column(call: &host::runtime::Call<'_>) -> marker::Column {
        marker::Column {
            witness: call.output_witness().unwrap(),
            identity: crate::widget::Identity::unique(),
            children: Vec::new(),
            mode: crate::widget::key::Mode::Positional,
            spacing: 0.0,
            padding: iced::Padding::ZERO,
            width: iced::Length::Fit,
            height: None,
            align_x: iced::Alignment::Start,
        }
    }

    fn text_application() -> &'static str {
        r#"
use iced::widget::{Text, text};
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
#[derive(Default)]
struct State { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Text { text(self.value) }
}
"#
    }

    fn button_application() -> &'static str {
        r#"
use iced::widget::{Button, button};
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
#[derive(Default)]
struct State { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Button<'_, Message> { button("Idle") }
}
"#
    }

    fn empty_column_application() -> &'static str {
        r#"
use iced::widget::{Column, column};
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
#[derive(Default)]
struct State { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Column<'_, Message> { column![] }
}
"#
    }

    fn failing_text_application() -> &'static str {
        r#"
use iced::widget::{Text, text};
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
#[derive(Default)]
struct State { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {
        match message { Message::Tick => self.value += 1 }
    }
    fn view(&self) -> Text { text(self.value).size(50) }
}
"#
    }

    fn column_application() -> &'static str {
        r#"
use iced::widget::{Column, column, text};
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
#[derive(Default)]
struct State;
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Column<'_, Message> {
        column![text("first"), text("second")]
    }
}
"#
    }

    fn keyed_column_application() -> &'static str {
        r#"
use iced::Element;
use iced::widget::{Column, column};
use rawr_iced::widget::key;
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
#[derive(Default)]
struct State;
struct Local { value: i64 }
struct Child { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
pub fn first_value() -> Element<Message> { Child { value: 10 } }
pub fn second_value() -> Element<Message> { Child { value: 20 } }
pub fn first() -> Element<Message> { key(first_value(), "first") }
pub fn second() -> Element<Message> { key(second_value(), "second") }
#[widget]
impl Child {
    fn state(&self) -> Local { Local { value: self.value } }
}
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Column<'_, Message> {
        column![first(), second()]
    }
}
"#
    }

    fn invalid_keyed_column_application(duplicate: bool) -> String {
        let second = if duplicate {
            "duplicate()"
        } else {
            "unkeyed()"
        };
        r#"
use iced::Element;
use iced::widget::{Column, column};
use rawr_iced::widget::key;
pub fn main() -> iced::Result {
    iced::run(State::update, State::view)
}
#[derive(Default)]
struct State;
struct Local { value: i64 }
struct Child { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Tick }
pub fn first_value() -> Element<Message> { Child { value: 10 } }
pub fn second_value() -> Element<Message> { Child { value: 20 } }
pub fn first() -> Element<Message> { key(first_value(), "first") }
pub fn unkeyed() -> Element<Message> { second_value() }
pub fn duplicate() -> Element<Message> { key(second_value(), "first") }
#[widget]
impl Child {
    fn state(&self) -> Local { Local { value: self.value } }
}
impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Column<'_, Message> {
        column![first(), $SECOND]
    }
}
"#
        .replace("$SECOND", second)
    }
}
