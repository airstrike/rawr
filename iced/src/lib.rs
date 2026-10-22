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
pub mod theme;
pub mod widget;

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

/// Build the validated compile-time `iced` facade contract.
pub fn facade() -> Result<facade::Definition, host::Error> {
    let mut builder = facade::Builder::new("iced", facade::Abi::new(16)?)?;
    builder.module("alignment")?;
    builder.module("animation")?;
    builder.module("border")?;
    builder.module("mouse")?;
    builder.module("padding")?;
    builder.module("theme")?;
    builder.module("theme::palette")?;
    builder.module("time")?;
    builder.module("widget")?;
    builder.module("widget::button")?;
    builder.module("widget::checkbox")?;
    builder.module("widget::combo_box")?;
    builder.module("widget::component")?;
    builder.module("widget::container")?;
    builder.module("widget::radio")?;
    builder.module("widget::scrollable")?;
    builder.module("widget::slider")?;
    builder.module("widget::text")?;
    builder.module("widget::text_input")?;
    builder.module("widget::toggler")?;
    builder.module("widget::tooltip")?;

    builder.type_::<marker::Result>("Result", &[], &[])?;
    builder.required_type::<task::Plan>("Task", &["Message"], &[])?;
    builder.stream_plan_type("Subscription", "Message")?;
    builder.value_type::<theme::Value>(
        "Theme",
        &[],
        &[capability::Kind::Clone, capability::Kind::PartialEq],
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
    builder.value_type::<iced::Color>("Color", &[], OWNED_VALUE_CAPABILITIES)?;
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
    builder.default_value_type::<iced::widget::tooltip::Position>(
        "widget::tooltip::Position",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<marker::Horizontal>("alignment::Horizontal", &[], OWNED_VALUE_CAPABILITIES)?;
    builder.default_value_type::<std::time::Duration>(
        "time::Duration",
        &[],
        OWNED_EQ_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<marker::Animation>("Animation", &[], OWNED_VALUE_CAPABILITIES)?;
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
    theme::bind(&mut iced_builder, &mut rawr_builder);
    widget::bind(&mut iced_builder, &mut rawr_builder);
    Ok([iced_builder.finish(), rawr_builder.finish()])
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

    builder.item(item::Definition::associated_function(
        "Animation::new",
        repeated_builtin_signature(type_ref::Builtin::F64, 1, "Animation")?,
    )?)?;
    for name in ["very_quick", "quick", "slow", "very_slow"] {
        builder.item(item::Definition::method(
            format!("Animation::{name}"),
            receiver::Mode::Owned,
            output_signature("Animation")?,
        )?)?;
    }
    builder.item(item::Definition::method(
        "Animation::easing",
        receiver::Mode::Owned,
        fixed_builder_signature("Animation", type_ref::Type::registered("animation::Easing")?)?,
    )?)?;
    let mut interpolate = signature::Builder::new();
    interpolate.parameter(type_ref::Type::registered("time::Instant")?);
    interpolate.output(type_ref::Type::builtin(type_ref::Builtin::F64));
    builder.item(item::Definition::method(
        "Animation::interpolate",
        receiver::Mode::Owned,
        interpolate.finish(),
    )?)?;
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
        type_ref::Type::registered("Animation")?,
    ));
    transition.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([
            type_ref::Type::registered("Animation")?,
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
    let parameter = type_ref::Type::parameter;
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
    if matches!(owner, "widget::Text" | "widget::Space" | "widget::ProgressBar") {
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
    for (name, input) in [
        ("background", color()?),
        ("border", border()?),
        ("color", color()?),
    ] {
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

const BUILT_IN_TEXT_STYLES: [&str; 6] = ["base", "primary", "secondary", "success", "warning", "danger"];
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
        let positional = boot(column_application());
        let context = positional.context();
        let positional = positional.display().lower(&context);
        let positional = Tree::new(&positional);
        assert_eq!(positional.tag, tree::Tag::stateless());

        let keyed = boot(keyed_column_application());
        let context = keyed.context();
        let keyed = keyed.display().lower(&context);
        let keyed = Tree::new(&keyed);
        assert_ne!(keyed.tag, tree::Tag::stateless());
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
            .bind_with_context("iced::widget::column", |call, args| {
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
