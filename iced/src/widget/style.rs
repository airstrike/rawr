//! Status-based styles: iced's `fn(&Theme, Status) -> Style` for button,
//! checkbox, radio, slider, scrollable, text_input and toggler.
//!
//! A style is a pinned iced function (`button::primary`) or a Rawr function
//! or closure. A Rawr function is evaluated once per status, so iced receives
//! a native function of the status. The table depends only on the function,
//! the theme and the code generation, so each owner computes it once and
//! reuses it until the theme changes; a new generation has a new owner.
//!
//! Each `Status` is a facade enum: a shared `bool` test per variant
//! (`Status::Active`) and a shared reader per named field (`is_checked`),
//! which `match` lowers to. Each `Style` has `default()` and one owned
//! builder per field, which struct literals lower to.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use iced::widget::{
    button, checkbox, container, progress_bar, radio, scrollable, slider, svg, text_input,
    toggler,
};
use iced::{Background, Border, Color, Shadow, Vector};
use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{receiver, Dynamic, EvalAltResult};

use super::builtin::{arity, pixels, runtime_error, take};
use crate::{runtime, theme, widget};

/// A widget's style: a pinned iced function, or a Rawr style function.
#[derive(Clone)]
pub(crate) enum Source<S, T> {
    Builtin(fn(&iced::Theme, S) -> T),
    Function(rawr::item::Value),
}

/// The value of a pinned iced style function, as `.style(button::primary)`.
pub(crate) struct Builtin<S, T>(fn(&iced::Theme, S) -> T);

impl<S, T> Clone for Builtin<S, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S, T> Copy for Builtin<S, T> {}

type Lowered<S, T> = Box<dyn Fn(&iced::Theme, S) -> T>;

/// iced's style function for `style`: the pinned one, or a Rawr function
/// evaluated for every status. `Ok(None)` without one or an authority.
pub(crate) fn lower<S, T>(
    style: &Option<Source<S, T>>,
    context: &widget::Context<'_>,
    statuses: &[S],
    widget: &str,
) -> Result<Option<Lowered<S, T>>, iced::Element<'static, runtime::Message>>
where
    S: Copy + PartialEq + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    let function = match style {
        None => return Ok(None),
        Some(Source::Builtin(style)) => return Ok(Some(Box::new(*style))),
        Some(Source::Function(function)) => function,
    };
    let table = match cached::<Vec<(S, T)>>(context, function) {
        Some(table) => table,
        None => {
            let theme = Dynamic::from(context.theme().clone());
            let mut table = Vec::with_capacity(statuses.len());
            for &status in statuses {
                let style =
                    match context.invoke(function, &mut [theme.clone(), Dynamic::from(status)]) {
                        None => return Ok(None),
                        Some(Ok(style)) => style.try_cast_result::<T>().map_err(|style| {
                            format!(
                                "expected a `{widget}::Style`, found `{}`",
                                style.type_name()
                            )
                        }),
                        Some(Err(error)) => Err(error),
                    };
                match style {
                    Ok(style) => table.push((status, style)),
                    Err(error) => {
                        return Err(
                            widget::diagnostic(format!("{widget} style failed: {error}"))
                                .lower(context),
                        )
                    }
                }
            }
            remember(context, function, table)
        }
    };
    Ok(Some(Box::new(move |_, status| {
        table
            .iter()
            .find(|(candidate, _)| *candidate == status)
            .unwrap_or(&table[0])
            .1
            .clone()
    })))
}

/// iced's style function for a progress bar: the pinned one, or a Rawr
/// function of the theme evaluated once per theme.
pub(crate) fn progress_bar_style(
    source: &Option<crate::marker::progress_bar::Source>,
    context: &widget::Context<'_>,
) -> Result<
    Option<Box<dyn Fn(&iced::Theme) -> progress_bar::Style>>,
    iced::Element<'static, runtime::Message>,
> {
    use crate::marker::progress_bar::Source;

    let function = match source {
        None => return Ok(None),
        Some(Source::Builtin(style)) => return Ok(Some(Box::new(*style))),
        Some(Source::Function(function)) => function,
    };
    let style = cached(context, function).map(Ok).or_else(|| {
        let theme = Dynamic::from(context.theme().clone());
        context.invoke(function, &mut [theme]).map(|style| {
            style.and_then(|mut style| {
                take::<progress_bar::Style>(&mut style, "iced::widget::progress_bar::Style")
                    .map(|style| remember(context, function, style))
                    .map_err(|error| error.to_string())
            })
        })
    });
    match style {
        Some(Ok(style)) => Ok(Some(Box::new(move |_: &iced::Theme| *style))),
        Some(Err(error)) => {
            Err(widget::diagnostic(format!("progress_bar style failed: {error}")).lower(context))
        }
        None => Ok(None),
    }
}

/// iced's pinned progress bar styles.
const PROGRESS_BAR_BUILTINS: [(&str, fn(&iced::Theme) -> progress_bar::Style); 5] = [
    ("primary", progress_bar::primary),
    ("secondary", progress_bar::secondary),
    ("success", progress_bar::success),
    ("warning", progress_bar::warning),
    ("danger", progress_bar::danger),
];

fn declare_progress_bar(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let style = || type_ref::Type::registered("widget::progress_bar::Style");
    let theme = || type_ref::Type::registered("Theme");
    builder.private_type::<crate::marker::progress_bar::Builtin>(
        "widget::progress_bar::Builtin",
        &[],
        &[],
    )?;
    for (name, _) in PROGRESS_BAR_BUILTINS {
        let path = format!("widget::progress_bar::{name}");
        builder.item(item::Definition::value(
            path.clone(),
            type_ref::Type::registered("widget::progress_bar::Builtin")?,
        )?)?;
        let mut function = signature::Builder::new();
        function.parameter(theme()?).output(style()?);
        builder.item(item::Definition::function(path, function.finish())?)?;
    }
    // iced's `impl Fn(&Theme) -> Style`: a pinned function, or a Rawr
    // function or closure.
    for input in [
        type_ref::Type::registered("widget::progress_bar::Builtin")?,
        type_ref::Type::callable(theme()?, style()?),
    ] {
        builder.item(item::Definition::method(
            "widget::ProgressBar::style",
            receiver::Mode::Owned,
            crate::builder_signature("widget::ProgressBar", input)?,
        )?)?;
    }
    Ok(())
}

fn bind_progress_bar(iced: &mut host::runtime::Builder) {
    use crate::marker::progress_bar::{Builtin, Source};

    for (name, style) in PROGRESS_BAR_BUILTINS {
        let path = format!("iced::widget::progress_bar::{name}");
        iced.bind(path.clone(), move |args| match args {
            [] => Ok(Dynamic::from(Builtin(style))),
            [theme] => {
                let theme = take::<crate::theme::Value>(theme, "iced::Theme")?;
                Ok(Dynamic::from(style(theme.native())))
            }
            _ => Err(arity(&path, 1, args.len())),
        })
        .expect("the iced facade must retain every pinned progress bar style");
    }
    iced.bind("iced::widget::ProgressBar::style", |args| {
        let [bar, style] = args else {
            return Err(arity("ProgressBar::style", 2, args.len()));
        };
        let mut bar = take::<crate::marker::ProgressBar>(bar, "iced::widget::ProgressBar")?;
        bar.style = Some(if style.is::<Builtin>() {
            Source::Builtin(take::<Builtin>(style, "style")?.0)
        } else {
            Source::Function(take(style, "style function")?)
        });
        Ok(Dynamic::from(bar))
    })
    .expect("the iced facade must retain ProgressBar::style");
    iced.bind("iced::widget::progress_bar::Style::default", |args| {
        if !args.is_empty() {
            return Err(arity("progress_bar::Style::default", 0, args.len()));
        }
        Ok(Dynamic::from(progress_bar::primary(&iced::Theme::Light)))
    })
    .expect("the iced facade must retain progress_bar::Style::default");
}

/// The style tables of one owner's generation, computed under one theme.
#[derive(Default)]
pub(crate) struct Tables {
    theme: Option<theme::Value>,
    tables: BTreeMap<rawr::item::Value, Arc<dyn Any + Send + Sync>>,
}

/// The table `function` produced under this context's theme.
pub(crate) fn cached<V: Any + Send + Sync>(
    context: &widget::Context<'_>,
    function: &rawr::item::Value,
) -> Option<Arc<V>> {
    // A closure's captures are new with each view: nothing to reuse.
    if !function.captures().is_empty() {
        return None;
    }
    let sink = context.diagnostics();
    let tables = sink.styles();
    if tables.theme.as_ref() != Some(context.theme()) {
        return None;
    }
    tables.tables.get(function)?.clone().downcast().ok()
}

/// Keep `table` for `function`, dropping tables from another theme.
pub(crate) fn remember<V: Any + Send + Sync>(
    context: &widget::Context<'_>,
    function: &rawr::item::Value,
    table: V,
) -> Arc<V> {
    let table = Arc::new(table);
    if !function.captures().is_empty() {
        return table;
    }
    let sink = context.diagnostics();
    let mut tables = sink.styles();
    if tables.theme.as_ref() != Some(context.theme()) {
        tables.theme = Some(context.theme().clone());
        tables.tables.clear();
    }
    tables.tables.insert(function.clone(), table.clone());
    table
}

/// Read one `.style(..)` argument: a pinned function or a Rawr function.
fn source<S: Clone + 'static, T: Clone + 'static>(
    value: &mut Dynamic,
) -> Result<Source<S, T>, Box<EvalAltResult>> {
    if value.is::<Builtin<S, T>>() {
        return Ok(Source::Builtin(take::<Builtin<S, T>>(value, "style")?.0));
    }
    Ok(Source::Function(take(value, "style function")?))
}

pub(crate) const BUTTON: [button::Status; 4] = [
    button::Status::Active,
    button::Status::Hovered,
    button::Status::Pressed,
    button::Status::Disabled,
];

pub(crate) const RADIO: [radio::Status; 4] = [
    radio::Status::Active { is_selected: false },
    radio::Status::Hovered { is_selected: false },
    radio::Status::Active { is_selected: true },
    radio::Status::Hovered { is_selected: true },
];

pub(crate) const SVG: [svg::Status; 2] = [svg::Status::Idle, svg::Status::Hovered];

pub(crate) const SLIDER: [slider::Status; 3] = [
    slider::Status::Active,
    slider::Status::Hovered,
    slider::Status::Dragged,
];

pub(crate) const TEXT_INPUT: [text_input::Status; 5] = [
    text_input::Status::Active,
    text_input::Status::Hovered,
    text_input::Status::Focused { is_hovered: false },
    text_input::Status::Focused { is_hovered: true },
    text_input::Status::Disabled,
];

pub(crate) fn checkbox_statuses() -> Vec<checkbox::Status> {
    [false, true]
        .into_iter()
        .flat_map(|is_checked| {
            [
                checkbox::Status::Active { is_checked },
                checkbox::Status::Hovered { is_checked },
                checkbox::Status::Disabled { is_checked },
            ]
        })
        .collect()
}

pub(crate) fn toggler_statuses() -> Vec<toggler::Status> {
    [false, true]
        .into_iter()
        .flat_map(|is_toggled| {
            [
                toggler::Status::Active { is_toggled },
                toggler::Status::Hovered { is_toggled },
                toggler::Status::Disabled { is_toggled },
            ]
        })
        .collect()
}

pub(crate) fn scrollable_statuses() -> Vec<scrollable::Status> {
    let mut statuses = Vec::with_capacity(36);
    for bits in 0..16_u8 {
        let [a, b, c, d] = [0, 1, 2, 3].map(|bit| bits & (1 << bit) != 0);
        if bits < 4 {
            statuses.push(scrollable::Status::Active {
                is_horizontal_scrollbar_disabled: a,
                is_vertical_scrollbar_disabled: b,
            });
        }
        statuses.push(scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered: a,
            is_vertical_scrollbar_hovered: b,
            is_horizontal_scrollbar_disabled: c,
            is_vertical_scrollbar_disabled: d,
        });
        statuses.push(scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged: a,
            is_vertical_scrollbar_dragged: b,
            is_horizontal_scrollbar_disabled: c,
            is_vertical_scrollbar_disabled: d,
        });
    }
    statuses
}

/// The widgets with status styles: widget type and its style module.
const WIDGETS: [(&str, &str); 7] = [
    ("widget::Button", "widget::button"),
    ("widget::Checkbox", "widget::checkbox"),
    ("widget::Radio", "widget::radio"),
    ("widget::Slider", "widget::slider"),
    ("widget::Scrollable", "widget::scrollable"),
    ("widget::TextInput", "widget::text_input"),
    ("widget::Toggler", "widget::toggler"),
];

/// Declare the status styles in the `iced` facade.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    builder.type_::<Background>("Background", &[], crate::OWNED_VALUE_CAPABILITIES)?;
    builder.type_::<Shadow>("Shadow", &[], crate::OWNED_DEFAULT_VALUE_CAPABILITIES)?;
    builder.type_::<button::Status>(
        "widget::button::Status",
        &[],
        crate::OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<checkbox::Status>(
        "widget::checkbox::Status",
        &[],
        crate::OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<radio::Status>(
        "widget::radio::Status",
        &[],
        crate::OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<slider::Status>(
        "widget::slider::Status",
        &[],
        crate::OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<scrollable::Status>(
        "widget::scrollable::Status",
        &[],
        crate::OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<text_input::Status>(
        "widget::text_input::Status",
        &[],
        crate::OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<toggler::Status>(
        "widget::toggler::Status",
        &[],
        crate::OWNED_VALUE_CAPABILITIES,
    )?;
    builder.type_::<svg::Status>("widget::svg::Status", &[], crate::OWNED_VALUE_CAPABILITIES)?;
    declare_types(builder)?;
    builder.private_type::<Builtin<button::Status, button::Style>>(
        "widget::button::Builtin",
        &[],
        &[],
    )?;
    builder.private_type::<Builtin<checkbox::Status, checkbox::Style>>(
        "widget::checkbox::Builtin",
        &[],
        &[],
    )?;
    builder.private_type::<Builtin<radio::Status, radio::Style>>(
        "widget::radio::Builtin",
        &[],
        &[],
    )?;
    builder.private_type::<Builtin<slider::Status, slider::Style>>(
        "widget::slider::Builtin",
        &[],
        &[],
    )?;
    builder.private_type::<Builtin<scrollable::Status, scrollable::Style>>(
        "widget::scrollable::Builtin",
        &[],
        &[],
    )?;
    builder.private_type::<Builtin<text_input::Status, text_input::Style>>(
        "widget::text_input::Builtin",
        &[],
        &[],
    )?;
    builder.private_type::<Builtin<toggler::Status, toggler::Style>>(
        "widget::toggler::Builtin",
        &[],
        &[],
    )?;

    for (owner, name) in STATUS_MEMBERS {
        let mut reader = signature::Builder::new();
        reader.output(type_ref::Type::builtin(type_ref::Builtin::Bool));
        builder.item(item::Definition::method(
            format!("{owner}::{name}"),
            receiver::Mode::Shared,
            reader.finish(),
        )?)?;
    }
    for path in DEFAULTS {
        let mut default = signature::Builder::new();
        default.output(type_ref::Type::registered(*path)?);
        builder.item(item::Definition::associated_function(
            format!("{path}::default"),
            default.finish(),
        )?)?;
    }
    declare_fields(builder)?;
    declare_handle_shapes(builder)?;
    declare_progress_bar(builder)?;
    let mut color = signature::Builder::new();
    color
        .parameter(type_ref::Type::registered("Color")?)
        .output(type_ref::Type::registered("Background")?);
    builder.item(item::Definition::associated_function(
        "Background::Color",
        color.finish(),
    )?)?;

    for (module, name) in BUILTINS {
        builder.item(item::Definition::value(
            format!("{module}::{name}"),
            type_ref::Type::registered(format!("{module}::Builtin"))?,
        )?)?;
        let mut function = signature::Builder::new();
        function
            .parameter(type_ref::Type::registered("Theme")?)
            .parameter(type_ref::Type::registered(format!("{module}::Status"))?)
            .output(type_ref::Type::registered(format!("{module}::Style"))?);
        builder.item(item::Definition::function(
            format!("{module}::{name}"),
            function.finish(),
        )?)?;
    }
    for (owner, module) in WIDGETS {
        // iced's `impl Fn(&Theme, Status) -> Style`: a pinned function, or
        // a Rawr function or closure.
        let callable = type_ref::Type::callable(
            type_ref::Type::tuple([
                type_ref::Type::registered("Theme")?,
                type_ref::Type::registered(format!("{module}::Status"))?,
            ])?,
            type_ref::Type::registered(format!("{module}::Style"))?,
        );
        for input in [
            type_ref::Type::registered(format!("{module}::Builtin"))?,
            callable,
        ] {
            let signature = if owner == "widget::Slider" {
                let mut signature = signature::Builder::new();
                signature.type_parameter("T", &[])?;
                signature.type_parameter("Message", &[])?;
                signature.parameter(input).output(crate::slider_output()?);
                signature.finish()
            } else {
                crate::builder_signature(owner, input)?
            };
            builder.item(item::Definition::method(
                format!("{owner}::style"),
                receiver::Mode::Owned,
                signature,
            )?)?;
        }
    }
    // iced's svg has no pinned style function: only a Rawr one.
    builder.item(item::Definition::method(
        "widget::Svg::style",
        receiver::Mode::Owned,
        crate::builder_signature(
            "widget::Svg",
            type_ref::Type::callable(
                type_ref::Type::tuple([
                    type_ref::Type::registered("Theme")?,
                    type_ref::Type::registered("widget::svg::Status")?,
                ])?,
                type_ref::Type::registered("widget::svg::Style")?,
            ),
        )?,
    )?)?;
    Ok(())
}

/// Bind the status styles in the `iced` runtime.
pub(crate) fn bind(iced: &mut host::runtime::Builder) {
    bind_statuses(iced);
    bind_fields(iced);
    bind_handle_shapes(iced);
    bind_progress_bar(iced);
    iced.bind("iced::Background::Color", |args| {
        if args.len() != 1 {
            return Err(arity("Background::Color", 1, args.len()));
        }
        Ok(Dynamic::from(Background::Color(take(
            args[0],
            "iced::Color",
        )?)))
    })
    .expect("the iced facade must retain Background::Color");
    bind_defaults(iced);
    bind_builtins(iced);
    bind_style::<crate::marker::Button, _, _>(iced, "Button", |button, style| {
        button.style = Some(style)
    });
    bind_style::<crate::marker::Checkbox, _, _>(iced, "Checkbox", |checkbox, style| {
        checkbox.style = Some(style)
    });
    bind_style::<crate::marker::Radio, _, _>(iced, "Radio", |radio, style| {
        radio.style = Some(style)
    });
    bind_style::<crate::marker::Slider, _, _>(iced, "Slider", |slider, style| {
        slider.style = Some(style)
    });
    bind_style::<crate::marker::Scrollable, _, _>(iced, "Scrollable", |scrollable, style| {
        scrollable.style = Some(style)
    });
    bind_style::<crate::marker::TextInput, _, _>(iced, "TextInput", |input, style| {
        input.style = Some(style)
    });
    bind_style::<crate::marker::Toggler, _, _>(iced, "Toggler", |toggler, style| {
        toggler.style = Some(style)
    });
    bind_style::<crate::marker::Svg, _, _>(iced, "Svg", |svg, style| svg.style = Some(style));
}

fn bind_style<W: Clone + Send + Sync + 'static, S: Clone + 'static, T: Clone + 'static>(
    iced: &mut host::runtime::Builder,
    widget: &'static str,
    set: fn(&mut W, Source<S, T>),
) {
    iced.bind(format!("iced::widget::{widget}::style"), move |args| {
        if args.len() != 2 {
            return Err(arity(widget, 2, args.len()));
        }
        let mut value = take::<W>(args[0], widget)?;
        set(&mut value, source(args[1])?);
        Ok(Dynamic::from(value))
    })
    .expect("the iced facade must retain every widget style builder");
}

/// A pinned iced style function: a value where it is passed, a call where
/// it is called, as `..button::primary(theme, status)`.
fn builtin<S, T>(
    iced: &mut host::runtime::Builder,
    path: &'static str,
    style: fn(&iced::Theme, S) -> T,
) where
    S: Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    iced.bind(path, move |args| match args.len() {
        0 => Ok(Dynamic::from(Builtin(style))),
        2 => {
            let theme = take::<crate::theme::Value>(args[0], "iced::Theme")?;
            let status = take::<S>(args[1], "Status")?;
            Ok(Dynamic::from(style(theme.native(), status)))
        }
        actual => Err(arity(path, 2, actual)),
    })
    .expect("the iced facade must retain every pinned style function");
}

const BUILTINS: [(&str, &str); 17] = [
    ("widget::button", "primary"),
    ("widget::button", "secondary"),
    ("widget::button", "success"),
    ("widget::button", "warning"),
    ("widget::button", "danger"),
    ("widget::button", "text"),
    ("widget::button", "background"),
    ("widget::button", "subtle"),
    ("widget::checkbox", "primary"),
    ("widget::checkbox", "secondary"),
    ("widget::checkbox", "success"),
    ("widget::checkbox", "danger"),
    ("widget::radio", "default"),
    ("widget::slider", "default"),
    ("widget::scrollable", "default"),
    ("widget::text_input", "default"),
    ("widget::toggler", "default"),
];

fn bind_builtins(iced: &mut host::runtime::Builder) {
    builtin(iced, "iced::widget::button::primary", button::primary);
    builtin(iced, "iced::widget::button::secondary", button::secondary);
    builtin(iced, "iced::widget::button::success", button::success);
    builtin(iced, "iced::widget::button::warning", button::warning);
    builtin(iced, "iced::widget::button::danger", button::danger);
    builtin(iced, "iced::widget::button::text", button::text);
    builtin(iced, "iced::widget::button::background", button::background);
    builtin(iced, "iced::widget::button::subtle", button::subtle);
    builtin(iced, "iced::widget::checkbox::primary", checkbox::primary);
    builtin(
        iced,
        "iced::widget::checkbox::secondary",
        checkbox::secondary,
    );
    builtin(iced, "iced::widget::checkbox::success", checkbox::success);
    builtin(iced, "iced::widget::checkbox::danger", checkbox::danger);
    builtin(iced, "iced::widget::radio::default", radio::default);
    builtin(iced, "iced::widget::slider::default", slider::default);
    builtin(
        iced,
        "iced::widget::scrollable::default",
        scrollable::default,
    );
    builtin(
        iced,
        "iced::widget::text_input::default",
        text_input::default,
    );
    builtin(iced, "iced::widget::toggler::default", toggler::default);
}

/// The style types with `default()`: iced's `Default` where it has one,
/// otherwise its default style for the light theme's resting status, so a
/// struct literal that names every field needs no `..base`.
const DEFAULTS: &[&str] = &[
    "Shadow",
    "widget::button::Style",
    "widget::checkbox::Style",
    "widget::radio::Style",
    "widget::slider::Style",
    "widget::slider::Rail",
    "widget::slider::Handle",
    "widget::slider::HandleShape::Circle",
    "widget::slider::HandleShape::Rectangle",
    "widget::scrollable::Style",
    "widget::scrollable::Rail",
    "widget::scrollable::Scroller",
    "widget::text_input::Style",
    "widget::toggler::Style",
    "widget::svg::Style",
    "widget::progress_bar::Style",
];

fn bind_defaults(iced: &mut host::runtime::Builder) {
    fn default<T: Clone + Send + Sync + 'static>(
        iced: &mut host::runtime::Builder,
        path: &'static str,
        value: fn() -> T,
    ) {
        iced.bind(path, move |args| {
            if !args.is_empty() {
                return Err(arity(path, 0, args.len()));
            }
            Ok(Dynamic::from(value()))
        })
        .expect("the iced facade must retain every style default");
    }
    default(iced, "iced::Shadow::default", Shadow::default);
    default(iced, "iced::widget::svg::Style::default", svg::Style::default);
    default(
        iced,
        "iced::widget::button::Style::default",
        button::Style::default,
    );
    default(iced, "iced::widget::checkbox::Style::default", || {
        checkbox::primary(
            &iced::Theme::Light,
            checkbox::Status::Active { is_checked: false },
        )
    });
    default(iced, "iced::widget::radio::Style::default", || {
        radio::default(
            &iced::Theme::Light,
            radio::Status::Active { is_selected: false },
        )
    });
    default(
        iced,
        "iced::widget::slider::HandleShape::Circle::default",
        Circle::default,
    );
    default(
        iced,
        "iced::widget::slider::HandleShape::Rectangle::default",
        Rectangle::default,
    );
    default(iced, "iced::widget::slider::Style::default", || {
        slider::default(&iced::Theme::Light, slider::Status::Active)
    });
    default(iced, "iced::widget::slider::Rail::default", || {
        slider::default(&iced::Theme::Light, slider::Status::Active).rail
    });
    default(iced, "iced::widget::slider::Handle::default", || {
        slider::default(&iced::Theme::Light, slider::Status::Active).handle
    });
    default(
        iced,
        "iced::widget::scrollable::Style::default",
        resting_scrollable,
    );
    default(iced, "iced::widget::scrollable::Rail::default", || {
        resting_scrollable().vertical_rail
    });
    default(iced, "iced::widget::scrollable::Scroller::default", || {
        resting_scrollable().vertical_rail.scroller
    });
    default(iced, "iced::widget::text_input::Style::default", || {
        text_input::default(&iced::Theme::Light, text_input::Status::Active)
    });
    default(iced, "iced::widget::toggler::Style::default", || {
        toggler::default(
            &iced::Theme::Light,
            toggler::Status::Active { is_toggled: false },
        )
    });
}

fn resting_scrollable() -> scrollable::Style {
    scrollable::default(
        &iced::Theme::Light,
        scrollable::Status::Active {
            is_horizontal_scrollbar_disabled: false,
            is_vertical_scrollbar_disabled: false,
        },
    )
}

/// `HandleShape::Circle { radius }` and `HandleShape::Rectangle { width,
/// border_radius }`: each struct variant is a facade struct literal of its
/// own, made the `HandleShape` by its `into` where the literal ends.
#[derive(Clone, Copy, Default)]
struct Circle {
    radius: f32,
}

#[derive(Clone, Copy, Default)]
struct Rectangle {
    width: f32,
    border_radius: iced::border::Radius,
}

fn declare_handle_shapes(builder: &mut facade::Builder) -> Result<(), host::Error> {
    for variant in ["Circle", "Rectangle"] {
        let mut into = signature::Builder::new();
        into.output(type_ref::Type::registered("widget::slider::HandleShape")?);
        builder.item(item::Definition::method(
            format!("widget::slider::HandleShape::{variant}::into"),
            receiver::Mode::Owned,
            into.finish(),
        )?)?;
    }
    Ok(())
}

fn bind_handle_shapes(iced: &mut host::runtime::Builder) {
    iced.bind("iced::widget::slider::HandleShape::Circle::into", |args| {
        let [circle] = args else {
            return Err(arity("HandleShape::Circle", 1, args.len()));
        };
        let Circle { radius } = take(circle, "iced::widget::slider::HandleShape::Circle")?;
        Ok(Dynamic::from(slider::HandleShape::Circle { radius }))
    })
    .expect("the iced facade must retain HandleShape::Circle");
    iced.bind("iced::widget::slider::HandleShape::Rectangle::into", |args| {
        let [rectangle] = args else {
            return Err(arity("HandleShape::Rectangle", 1, args.len()));
        };
        let Rectangle {
            width,
            border_radius,
        } = take(rectangle, "iced::widget::slider::HandleShape::Rectangle")?;
        Ok(Dynamic::from(slider::HandleShape::Rectangle {
            width: width as u16,
            border_radius,
        }))
    })
    .expect("the iced facade must retain HandleShape::Rectangle");
}

/// A style field's facade inputs and how its builder reads one; and, when
/// the field reads as a facade value, its type and value.
trait Field: Sized {
    fn inputs() -> Result<Vec<type_ref::Type>, host::Error>;
    fn read(value: &mut Dynamic) -> Result<Self, Box<EvalAltResult>>;
    fn output() -> Result<Option<type_ref::Type>, host::Error> {
        Ok(Self::inputs()?.into_iter().next())
    }
    fn write(&self) -> Dynamic;
}

macro_rules! registered {
    ($($ty:ty => $path:literal),* $(,)?) => {$(
        impl Field for $ty {
            fn inputs() -> Result<Vec<type_ref::Type>, host::Error> {
                Ok(vec![type_ref::Type::registered($path)?])
            }
            fn read(value: &mut Dynamic) -> Result<Self, Box<EvalAltResult>> {
                take(value, concat!("iced::", $path))
            }
            fn write(&self) -> Dynamic {
                Dynamic::from(*self)
            }
        }
    )*};
}

registered! {
    Color => "Color",
    Border => "Border",
    Shadow => "Shadow",
    Vector => "Vector",
    container::Style => "widget::container::Style",
    slider::Rail => "widget::slider::Rail",
    slider::Handle => "widget::slider::Handle",
    slider::HandleShape => "widget::slider::HandleShape",
    scrollable::Rail => "widget::scrollable::Rail",
    scrollable::Scroller => "widget::scrollable::Scroller",
}

/// iced's `impl Into<Background>`: a `Background` or a `Color`.
impl Field for Background {
    fn inputs() -> Result<Vec<type_ref::Type>, host::Error> {
        Ok(vec![
            type_ref::Type::registered("Background")?,
            type_ref::Type::registered("Color")?,
        ])
    }
    fn read(value: &mut Dynamic) -> Result<Self, Box<EvalAltResult>> {
        if value.is::<Color>() {
            return take(value, "iced::Color").map(Background::Color);
        }
        take(value, "iced::Background")
    }
    fn write(&self) -> Dynamic {
        Dynamic::from(*self)
    }
}

/// An optional field takes and reads an `Option`, as in Rust:
/// `background: Some(Background::Color(c))` or `Some(color.into())`, an
/// `Option` of any input of the field. It also still takes a bare value, as
/// before Rawr had `Option`.
impl<T: Field> Field for Option<T> {
    fn inputs() -> Result<Vec<type_ref::Type>, host::Error> {
        let bare = T::inputs()?;
        let mut inputs = bare.clone();
        if let Some(output) = T::output()?
            && !bare.contains(&output)
        {
            inputs.push(type_ref::Type::option(output));
        }
        inputs.extend(bare.into_iter().map(type_ref::Type::option));
        Ok(inputs)
    }
    fn read(value: &mut Dynamic) -> Result<Self, Box<EvalAltResult>> {
        match rawr::option::take(value) {
            Some(Some(mut inner)) => T::read(&mut inner).map(Some),
            Some(None) => Ok(None),
            None => T::read(value).map(Some),
        }
    }
    fn output() -> Result<Option<type_ref::Type>, host::Error> {
        Ok(T::output()?.map(type_ref::Type::option))
    }
    fn write(&self) -> Dynamic {
        match self {
            Some(value) => rawr::option::some(value.write()),
            None => rawr::option::none(),
        }
    }
}

impl Field for f32 {
    fn inputs() -> Result<Vec<type_ref::Type>, host::Error> {
        Ok(crate::PIXELS.map(type_ref::Type::builtin).to_vec())
    }
    fn read(value: &mut Dynamic) -> Result<Self, Box<EvalAltResult>> {
        pixels(value, "style field")
    }
    fn output() -> Result<Option<type_ref::Type>, host::Error> {
        Ok(Some(type_ref::Type::builtin(type_ref::Builtin::F64)))
    }
    fn write(&self) -> Dynamic {
        Dynamic::from(f64::from(*self))
    }
}

/// iced's `impl Into<Radius>`: a `Radius`, or uniform pixels.
impl Field for iced::border::Radius {
    fn inputs() -> Result<Vec<type_ref::Type>, host::Error> {
        let mut inputs = vec![type_ref::Type::registered("border::Radius")?];
        inputs.extend(f32::inputs()?);
        Ok(inputs)
    }
    fn read(value: &mut Dynamic) -> Result<Self, Box<EvalAltResult>> {
        if value.is::<Self>() {
            return take(value, "iced::border::Radius");
        }
        f32::read(value).map(Self::from)
    }
    fn write(&self) -> Dynamic {
        Dynamic::from(*self)
    }
}

impl Field for bool {
    fn inputs() -> Result<Vec<type_ref::Type>, host::Error> {
        Ok(vec![type_ref::Type::builtin(type_ref::Builtin::Bool)])
    }
    fn read(value: &mut Dynamic) -> Result<Self, Box<EvalAltResult>> {
        value
            .as_bool()
            .map_err(|_| runtime_error(format!("expected `bool`, found `{}`", value.type_name())))
    }
    fn write(&self) -> Dynamic {
        Dynamic::from(*self)
    }
}

macro_rules! fields {
    ($($path:literal => $ty:ty { $($field:ident: $field_ty:ty),* $(,)? })*) => {
        /// One owned builder per field, as `Style { field: x, ..base }`, and
        /// a shared reader, as `base.field`.
        fn declare_fields(builder: &mut facade::Builder) -> Result<(), host::Error> {
            $($(
                if let Some(output) = <$field_ty as Field>::output()? {
                    let mut reader = signature::Builder::new();
                    reader.output(output);
                    builder.item(item::Definition::method(
                        concat!($path, "::", stringify!($field)),
                        receiver::Mode::Shared,
                        reader.finish(),
                    )?)?;
                }
                for input in <$field_ty as Field>::inputs()? {
                    let mut method = signature::Builder::new();
                    method.parameter(input).output(type_ref::Type::registered($path)?);
                    builder.item(item::Definition::method(
                        concat!($path, "::", stringify!($field)),
                        receiver::Mode::Owned,
                        method.finish(),
                    )?)?;
                }
            )*)*
            Ok(())
        }

        fn bind_fields(iced: &mut host::runtime::Builder) {
            $($(
                iced.bind(concat!("iced::", $path, "::", stringify!($field)), |args| {
                    if let [style] = args {
                        let style = style.read_lock::<$ty>().ok_or_else(|| {
                            runtime_error(format!("expected `{}`", $path))
                        })?;
                        return Ok(<$field_ty as Field>::write(&style.$field));
                    }
                    if args.len() != 2 {
                        return Err(arity(stringify!($field), 2, args.len()));
                    }
                    let mut style = take::<$ty>(args[0], concat!("iced::", $path))?;
                    style.$field = <$field_ty as Field>::read(args[1])?;
                    Ok(Dynamic::from(style))
                })
                .expect("the iced facade must retain every style field");
            )*)*
        }
    };
}

fields! {
    "Border" => Border { radius: iced::border::Radius }
    "border::Radius" => iced::border::Radius {
        top_left: f32,
        top_right: f32,
        bottom_right: f32,
        bottom_left: f32,
    }
    "widget::svg::Style" => svg::Style { color: Option<Color> }
    "widget::container::Style" => container::Style {
        background: Option<Background>,
        text_color: Option<Color>,
        shadow: Shadow,
        snap: bool,
    }
    "Shadow" => Shadow { color: Color, offset: Vector, blur_radius: f32 }
    "widget::button::Style" => button::Style {
        background: Option<Background>,
        text_color: Color,
        border: Border,
        shadow: Shadow,
        snap: bool,
    }
    "widget::checkbox::Style" => checkbox::Style {
        background: Background,
        icon_color: Color,
        border: Border,
        text_color: Option<Color>,
    }
    "widget::slider::Style" => slider::Style { rail: slider::Rail, handle: slider::Handle }
    "widget::slider::Rail" => slider::Rail { width: f32, border: Border }
    "widget::slider::Handle" => slider::Handle {
        shape: slider::HandleShape,
        background: Background,
        border_width: f32,
        border_color: Color,
    }
    "widget::slider::HandleShape::Circle" => Circle { radius: f32 }
    "widget::slider::HandleShape::Rectangle" => Rectangle {
        width: f32,
        border_radius: iced::border::Radius,
    }
    "widget::radio::Style" => radio::Style {
        background: Background,
        dot_color: Color,
        border_width: f32,
        border_color: Color,
        text_color: Option<Color>,
    }
    "widget::scrollable::Style" => scrollable::Style {
        container: container::Style,
        vertical_rail: scrollable::Rail,
        horizontal_rail: scrollable::Rail,
        gap: Option<Background>,
    }
    "widget::scrollable::Rail" => scrollable::Rail {
        background: Option<Background>,
        border: Border,
        scroller: scrollable::Scroller,
    }
    "widget::scrollable::Scroller" => scrollable::Scroller { background: Background, border: Border }
    "widget::progress_bar::Style" => progress_bar::Style {
        background: Background,
        bar: Background,
        border: Border,
    }
    "widget::text_input::Style" => text_input::Style {
        background: Background,
        border: Border,
        placeholder: Color,
        value: Color,
        selection: Color,
    }
    "widget::toggler::Style" => toggler::Style {
        background: Background,
        background_border_width: f32,
        background_border_color: Color,
        foreground: Background,
        foreground_border_width: f32,
        foreground_border_color: Color,
        text_color: Option<Color>,
        border_radius: Option<iced::border::Radius>,
        padding_ratio: f32,
    }
}

/// Register the style types the fields name.
fn declare_types(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let owned = crate::OWNED_VALUE_CAPABILITIES;
    builder.type_::<iced::border::Radius>(
        "border::Radius",
        &[],
        crate::OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.type_::<button::Style>(
        "widget::button::Style",
        &[],
        crate::OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    builder.type_::<checkbox::Style>("widget::checkbox::Style", &[], owned)?;
    builder.type_::<slider::Style>("widget::slider::Style", &[], owned)?;
    builder.type_::<slider::Rail>("widget::slider::Rail", &[], owned)?;
    builder.type_::<slider::Handle>("widget::slider::Handle", &[], owned)?;
    builder.type_::<slider::HandleShape>("widget::slider::HandleShape", &[], owned)?;
    builder.type_::<Circle>("widget::slider::HandleShape::Circle", &[], owned)?;
    builder.type_::<Rectangle>("widget::slider::HandleShape::Rectangle", &[], owned)?;
    builder.type_::<radio::Style>("widget::radio::Style", &[], owned)?;
    builder.type_::<scrollable::Style>("widget::scrollable::Style", &[], owned)?;
    builder.type_::<scrollable::Rail>("widget::scrollable::Rail", &[], owned)?;
    builder.type_::<scrollable::Scroller>("widget::scrollable::Scroller", &[], owned)?;
    builder.type_::<text_input::Style>("widget::text_input::Style", &[], owned)?;
    builder.type_::<toggler::Style>("widget::toggler::Style", &[], owned)?;
    builder.type_::<progress_bar::Style>("widget::progress_bar::Style", &[], owned)?;
    builder.type_::<svg::Style>(
        "widget::svg::Style",
        &[],
        crate::OWNED_DEFAULT_VALUE_CAPABILITIES,
    )?;
    Ok(())
}

macro_rules! statuses {
    ($($path:literal => $module:ident { $($member:ident: |$status:ident| $test:expr),* $(,)? })*) => {
        /// Each `Status` variant test and named-field reader.
        const STATUS_MEMBERS: &[(&str, &str)] = &[$($(($path, stringify!($member)),)*)*];

        fn bind_statuses(iced: &mut host::runtime::Builder) {
            $($(
                iced.bind(concat!("iced::", $path, "::", stringify!($member)), |args| {
                    let [value] = args else {
                        return Err(arity(stringify!($member), 1, args.len()));
                    };
                    let $status = value.clone().try_cast_result::<$module::Status>().map_err(|value| {
                        runtime_error(format!("expected `{}`, found `{}`", $path, value.type_name()))
                    })?;
                    #[allow(unused_imports)]
                    use $module::Status::*;
                    Ok(Dynamic::from($test))
                })
                .expect("the iced facade must retain every Status member");
            )*)*
        }
    };
}

statuses! {
    "widget::svg::Status" => svg {
        Idle: |status| status == Idle,
        Hovered: |status| status == Hovered,
    }
    "widget::button::Status" => button {
        Active: |status| status == Active,
        Hovered: |status| status == Hovered,
        Pressed: |status| status == Pressed,
        Disabled: |status| status == Disabled,
    }
    "widget::radio::Status" => radio {
        Active: |status| matches!(status, Active { .. }),
        Hovered: |status| matches!(status, Hovered { .. }),
        is_selected: |status| match status {
            Active { is_selected } | Hovered { is_selected } => is_selected,
        },
    }
    "widget::slider::Status" => slider {
        Active: |status| status == Active,
        Hovered: |status| status == Hovered,
        Dragged: |status| status == Dragged,
    }
    "widget::text_input::Status" => text_input {
        Active: |status| status == Active,
        Hovered: |status| status == Hovered,
        Focused: |status| matches!(status, Focused { .. }),
        Disabled: |status| status == Disabled,
        is_hovered: |status| matches!(status, Focused { is_hovered: true }),
    }
    "widget::checkbox::Status" => checkbox {
        Active: |status| matches!(status, Active { .. }),
        Hovered: |status| matches!(status, Hovered { .. }),
        Disabled: |status| matches!(status, Disabled { .. }),
        is_checked: |status| match status {
            Active { is_checked } | Hovered { is_checked } | Disabled { is_checked } => is_checked,
        },
    }
    "widget::toggler::Status" => toggler {
        Active: |status| matches!(status, Active { .. }),
        Hovered: |status| matches!(status, Hovered { .. }),
        Disabled: |status| matches!(status, Disabled { .. }),
        is_toggled: |status| match status {
            Active { is_toggled } | Hovered { is_toggled } | Disabled { is_toggled } => is_toggled,
        },
    }
    "widget::scrollable::Status" => scrollable {
        Active: |status| matches!(status, Active { .. }),
        Hovered: |status| matches!(status, Hovered { .. }),
        Dragged: |status| matches!(status, Dragged { .. }),
        is_horizontal_scrollbar_hovered: |status| matches!(
            status,
            Hovered { is_horizontal_scrollbar_hovered: true, .. }
        ),
        is_vertical_scrollbar_hovered: |status| matches!(
            status,
            Hovered { is_vertical_scrollbar_hovered: true, .. }
        ),
        is_horizontal_scrollbar_dragged: |status| matches!(
            status,
            Dragged { is_horizontal_scrollbar_dragged: true, .. }
        ),
        is_vertical_scrollbar_dragged: |status| matches!(
            status,
            Dragged { is_vertical_scrollbar_dragged: true, .. }
        ),
        is_horizontal_scrollbar_disabled: |status| match status {
            Active { is_horizontal_scrollbar_disabled, .. }
            | Hovered { is_horizontal_scrollbar_disabled, .. }
            | Dragged { is_horizontal_scrollbar_disabled, .. } => is_horizontal_scrollbar_disabled,
        },
        is_vertical_scrollbar_disabled: |status| match status {
            Active { is_vertical_scrollbar_disabled, .. }
            | Hovered { is_vertical_scrollbar_disabled, .. }
            | Dragged { is_vertical_scrollbar_disabled, .. } => is_vertical_scrollbar_disabled,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use iced::widget::slider;

    use super::{cached, lower, Source, SLIDER};

    #[test]
    fn lowering_again_under_the_same_theme_reuses_the_style_table() {
        let mut engine = rawr::Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = rawr::Shared::new(engine);
        let candidate = engine
            .compile_application(
                r#"
use iced::widget::{slider, text};
use iced::widget::slider::{Status, Style};
use iced::{Element, Theme};

struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("styled").boxed() }
}

fn fader(theme: &Theme, status: Status) -> Style { slider::default(theme, status) }

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#,
            )
            .unwrap();
        let function = candidate
            .items()
            .find(|item| item.key().name() == "fader")
            .unwrap()
            .clone();
        let launch = engine
            .launch_application(candidate.validate().unwrap())
            .unwrap();
        let mut registry = rawr::host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = Arc::new(launch.bind(registry).unwrap());
        let instance = rawr::application::Instance::new();
        let sink = Arc::new(crate::widget::diagnostic::Sink::new(
            instance,
            launch.generation(),
        ));
        let style = Some(Source::<slider::Status, slider::Style>::Function(
            function.clone(),
        ));
        let table = |theme: &crate::theme::Value| {
            let context = crate::widget::Context::application(&launch, instance, &sink, theme);
            assert!(matches!(
                lower(&style, &context, &SLIDER, "slider"),
                Ok(Some(_))
            ));
            cached::<Vec<(slider::Status, slider::Style)>>(&context, &function).unwrap()
        };

        let light = crate::theme::Value::default();
        let first = table(&light);
        assert!(Arc::ptr_eq(&first, &table(&light)));

        let palette = crate::theme::Palette::from_hex(
            "#101010", "#f0f0f0", "#e04080", "#40c080", "#e0a040", "#e04040",
        )
        .unwrap();
        let ink = crate::theme::Value::custom("Ink", palette);
        assert!(!Arc::ptr_eq(&first, &table(&ink)));
    }
}
