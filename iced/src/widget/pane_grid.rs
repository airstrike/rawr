//! iced's `pane_grid` for the playground (`features::playground`).
//!
//! A `?play` snippet is no Cargo project, so it reaches no importer, and
//! iced's pane_grid example would name nothing here. `rawr run` reaches
//! `pane_grid` through the importer (D558 tier 2) and never declares these.
//! Under D550 they are facade debt, as D548's backend picker is, and go
//! once the playground imports its crates (D513.5).
//!
//! The `pane_grid::State<T>` of script values lives in the application's
//! native home and the script's value holds it by handle, as
//! `text_editor::Content` does (D546): `update` changes it in place, and a
//! `PaneGrid` borrows it for iced's view lifetime. `get_mut` lends a pane's
//! value as a place (`rawr::place`), so a pattern's `&mut` binding
//! writes back into the State. A `PaneGrid` keeps its pane closure, which
//! runs for each pane when the grid lowers, as `responsive`'s view does.

use std::cell::RefCell;
use std::collections::HashMap;

use iced::widget::pane_grid as native;
use iced::widget::Widget as _;
use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{capability, receiver, Dynamic, EvalAltResult, Position};

use super::builtin::{
    arity, container_style, length, number, padding, pixels, take, witness_error, Witnessed,
};
use super::helpers::bind_widget_type;
use crate::{home, marker, runtime, widget};

type NativeState = native::State<Dynamic>;
type Element<'a> = iced::Element<'a, runtime::Message>;

/// `pane_grid::State<T>`: the handle of iced's state in the native home.
#[derive(Clone)]
pub(crate) struct State {
    witness: host::runtime::Witness,
    native: home::Handle,
}

/// iced's `PaneGrid<Message>`: the State it borrows, its pane closure and
/// its builders.
#[derive(Clone)]
pub(crate) struct Grid {
    witness: host::runtime::Witness,
    identity: widget::Identity,
    state: home::Handle,
    /// A Rawr `Fn(Pane, &T, bool) -> Content<Message>`.
    view: rawr::item::Value,
    width: iced::Length,
    height: iced::Length,
    spacing: f32,
    min_size: Option<f32>,
    on_click: Option<rawr::item::Value>,
    on_drag: Option<rawr::item::Value>,
    on_resize: Option<(f32, rawr::item::Value)>,
}

/// `pane_grid::Content<Message>`: a pane's body, title bar and style.
#[derive(Clone)]
pub(crate) struct Content {
    witness: host::runtime::Witness,
    body: widget::Value,
    title_bar: Option<TitleBar>,
    style: Option<marker::container::Source>,
}

/// `pane_grid::TitleBar<Message>`.
#[derive(Clone)]
pub(crate) struct TitleBar {
    witness: host::runtime::Witness,
    content: widget::Value,
    controls: Option<Controls>,
    padding: Option<iced::Padding>,
    always_show_controls: bool,
    style: Option<marker::container::Source>,
}

/// `pane_grid::Controls<Message>`: the full controls, and the compact ones
/// shown when the full ones don't fit.
#[derive(Clone)]
pub(crate) struct Controls {
    witness: host::runtime::Witness,
    full: widget::Value,
    compact: Option<widget::Value>,
}

const MODULE: &str = "widget::pane_grid";
const STATE: &str = "widget::pane_grid::State";
const GRID: &str = "widget::PaneGrid";
const CONTENT: &str = "widget::pane_grid::Content";
const TITLE_BAR: &str = "widget::pane_grid::TitleBar";
const CONTROLS: &str = "widget::pane_grid::Controls";

/// A `Copy` value iced derives `Debug` for but not `PartialEq`, as
/// `DragEvent`.
const EVENT_CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Copy,
    capability::Kind::Send,
    capability::Kind::Static,
];

const AXES: [(&str, native::Axis); 2] = [
    ("Horizontal", native::Axis::Horizontal),
    ("Vertical", native::Axis::Vertical),
];
const DIRECTIONS: [(&str, native::Direction); 4] = [
    ("Up", native::Direction::Up),
    ("Down", native::Direction::Down),
    ("Left", native::Direction::Left),
    ("Right", native::Direction::Right),
];
const EDGES: [(&str, native::Edge); 4] = [
    ("Top", native::Edge::Top),
    ("Left", native::Edge::Left),
    ("Right", native::Edge::Right),
    ("Bottom", native::Edge::Bottom),
];

fn ty(name: &str) -> Result<type_ref::Type, host::Error> {
    type_ref::Type::registered(format!("{MODULE}::{name}"))
}

fn message() -> Result<type_ref::Type, host::Error> {
    type_ref::Type::parameter("Message")
}

fn of_message(owner: &str) -> Result<type_ref::Type, host::Error> {
    type_ref::Type::applied(owner, [message()?])
}

fn element() -> Result<type_ref::Type, host::Error> {
    of_message("Element")
}

/// One `Message`-generic item of `owner` with `inputs`, giving `output`.
fn generic(
    builder: &mut facade::Builder,
    path: String,
    kind: receiver::Mode,
    inputs: impl IntoIterator<Item = type_ref::Type>,
    output: type_ref::Type,
) -> Result<(), host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    for input in inputs {
        signature.parameter(input);
    }
    signature.output(output);
    builder.item(item::Definition::method(path, kind, signature.finish())?)?;
    Ok(())
}

/// `Content::new(body)`, `TitleBar::new(content)` and `Controls::new` or
/// `dynamic`: an associated function of `Message`.
fn constructor(
    builder: &mut facade::Builder,
    path: &str,
    inputs: usize,
    output: &str,
) -> Result<(), host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("Message", &[])?;
    for _ in 0..inputs {
        signature.parameter(element()?);
    }
    signature.output(of_message(output)?);
    builder.item(item::Definition::associated_function(
        path,
        signature.finish(),
    )?)?;
    Ok(())
}

/// A `State<T>` method with `inputs`, giving `output`.
fn state_method(
    builder: &mut facade::Builder,
    name: &str,
    receiver: receiver::Mode,
    inputs: impl IntoIterator<Item = type_ref::Type>,
    output: type_ref::Type,
    guard: Option<item::Guard>,
) -> Result<(), host::Error> {
    let mut signature = signature::Builder::new();
    signature.type_parameter("T", &[])?;
    for input in inputs {
        signature.parameter(input);
    }
    signature.output(output);
    builder.item(
        item::Definition::method(format!("{STATE}::{name}"), receiver, signature.finish())?
            .guarded(guard),
    )?;
    Ok(())
}

/// A reader of a facade enum or struct, as a pattern reads it.
fn reader(
    builder: &mut facade::Builder,
    path: &str,
    output: type_ref::Type,
) -> Result<(), host::Error> {
    let mut signature = signature::Builder::new();
    signature.output(output);
    builder.item(item::Definition::method(
        format!("{MODULE}::{path}"),
        receiver::Mode::Shared,
        signature.finish(),
    )?)?;
    Ok(())
}

/// Declare iced's `pane_grid` on the playground's facade.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use rawr::host::debug::register;
    register::<native::Pane>();
    register::<native::Split>();
    register::<native::Axis>();
    register::<native::Direction>();
    register::<native::DragEvent>();
    register::<native::ResizeEvent>();
    register::<native::Target>();
    register::<native::Region>();
    register::<native::Edge>();

    builder.module(MODULE)?;
    builder.value_type::<native::Pane>(
        format!("{MODULE}::Pane"),
        &[],
        crate::OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<native::Split>(
        format!("{MODULE}::Split"),
        &[],
        crate::OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<native::Axis>(
        format!("{MODULE}::Axis"),
        &[],
        crate::OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    builder.value_type::<native::Direction>(
        format!("{MODULE}::Direction"),
        &[],
        crate::OWNED_EQ_VALUE_CAPABILITIES,
    )?;
    for name in ["DragEvent", "ResizeEvent", "Target", "Region", "Edge"] {
        let path = format!("{MODULE}::{name}");
        match name {
            "DragEvent" => builder.type_::<native::DragEvent>(path, &[], EVENT_CAPABILITIES)?,
            "ResizeEvent" => builder.type_::<native::ResizeEvent>(path, &[], EVENT_CAPABILITIES)?,
            "Target" => builder.type_::<native::Target>(path, &[], EVENT_CAPABILITIES)?,
            "Region" => builder.type_::<native::Region>(path, &[], EVENT_CAPABILITIES)?,
            _ => builder.type_::<native::Edge>(path, &[], EVENT_CAPABILITIES)?,
        };
    }
    builder.type_::<State>(STATE, &["T"], &[])?;
    builder.type_::<Grid>(GRID, &["Message"], &[])?;
    builder.project(GRID, "Element", &["Message"])?;
    builder.alias(format!("{MODULE}::PaneGrid"), GRID)?;
    builder.type_::<Content>(CONTENT, &["Message"], &[])?;
    builder.type_::<TitleBar>(TITLE_BAR, &["Message"], &[])?;
    builder.type_::<Controls>(CONTROLS, &["Message"], &[])?;

    for (name, _) in AXES {
        builder.item(item::Definition::associated_value(
            format!("{MODULE}::Axis::{name}"),
            ty("Axis")?,
        )?)?;
    }
    for (name, _) in DIRECTIONS {
        builder.item(item::Definition::associated_value(
            format!("{MODULE}::Direction::{name}"),
            ty("Direction")?,
        )?)?;
    }
    for (name, _) in EDGES {
        builder.item(item::Definition::associated_value(
            format!("{MODULE}::Edge::{name}"),
            ty("Edge")?,
        )?)?;
    }
    builder.item(item::Definition::associated_value(
        format!("{MODULE}::Region::Center"),
        ty("Region")?,
    )?)?;
    // `DragEvent::Dropped { pane, target }` and `ResizeEvent { split,
    // ratio }` in a pattern.
    let bool_ = || type_ref::Type::builtin(type_ref::Builtin::Bool);
    for variant in ["Picked", "Dropped", "Canceled"] {
        reader(builder, &format!("DragEvent::{variant}"), bool_())?;
    }
    reader(builder, "DragEvent::pane", ty("Pane")?)?;
    reader(builder, "DragEvent::target", ty("Target")?)?;
    reader(builder, "ResizeEvent::split", ty("Split")?)?;
    reader(
        builder,
        "ResizeEvent::ratio",
        type_ref::Type::builtin(type_ref::Builtin::F64),
    )?;

    declare_state(builder)?;
    declare_grid(builder)?;
    declare_parts(builder)
}

/// `State::new` and the methods `update` calls.
fn declare_state(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let t = || type_ref::Type::parameter("T");
    let state = || type_ref::Type::applied(STATE, [t()?]);
    let pane = || ty("Pane");
    let mut new = signature::Builder::new();
    new.type_parameter("T", &[])?;
    new.parameter(t()?);
    new.output(type_ref::Type::tuple([state()?, pane()?])?);
    builder.item(item::Definition::associated_function(
        format!("{STATE}::new"),
        new.finish(),
    )?)?;
    let usize_ = || type_ref::Type::builtin(type_ref::Builtin::Usize);
    let unit = type_ref::Type::unit;
    let shared = receiver::Mode::Shared;
    let mutable = receiver::Mode::Mutable;
    state_method(builder, "len", shared, [], usize_(), None)?;
    state_method(
        builder,
        "is_empty",
        shared,
        [],
        type_ref::Type::builtin(type_ref::Builtin::Bool),
        None,
    )?;
    state_method(
        builder,
        "get",
        shared,
        [pane()?],
        type_ref::Type::option(t()?),
        Some(item::Guard::OptionShared),
    )?;
    state_method(
        builder,
        "get_mut",
        mutable,
        [pane()?],
        type_ref::Type::option(t()?),
        Some(item::Guard::OptionMutable),
    )?;
    state_method(
        builder,
        "adjacent",
        shared,
        [pane()?, ty("Direction")?],
        type_ref::Type::option(pane()?),
        None,
    )?;
    state_method(
        builder,
        "maximized",
        shared,
        [],
        type_ref::Type::option(pane()?),
        None,
    )?;
    state_method(
        builder,
        "split",
        mutable,
        [ty("Axis")?, pane()?, t()?],
        type_ref::Type::option(type_ref::Type::tuple([pane()?, ty("Split")?])?),
        None,
    )?;
    state_method(
        builder,
        "close",
        mutable,
        [pane()?],
        type_ref::Type::option(type_ref::Type::tuple([t()?, pane()?])?),
        None,
    )?;
    state_method(
        builder,
        "drop",
        mutable,
        [pane()?, ty("Target")?],
        unit(),
        None,
    )?;
    state_method(builder, "swap", mutable, [pane()?, pane()?], unit(), None)?;
    state_method(
        builder,
        "move_to_edge",
        mutable,
        [pane()?, ty("Edge")?],
        unit(),
        None,
    )?;
    state_method(
        builder,
        "resize",
        mutable,
        [
            ty("Split")?,
            type_ref::Type::builtin(type_ref::Builtin::F64),
        ],
        unit(),
        None,
    )?;
    state_method(builder, "maximize", mutable, [pane()?], unit(), None)?;
    state_method(builder, "restore", mutable, [], unit(), None)?;
    Ok(())
}

/// `PaneGrid::new(&state, view)` and its builders.
fn declare_grid(builder: &mut facade::Builder) -> Result<(), host::Error> {
    let t = || type_ref::Type::parameter("T");
    let mut new = signature::Builder::new();
    new.type_parameter("T", &[])?;
    new.type_parameter("Message", &[])?;
    new.parameter(type_ref::Type::applied(STATE, [t()?])?);
    new.parameter(type_ref::Type::callable(
        type_ref::Type::tuple([
            ty("Pane")?,
            t()?,
            type_ref::Type::builtin(type_ref::Builtin::Bool),
        ])?,
        of_message(CONTENT)?,
    ));
    new.output(of_message(GRID)?);
    builder.item(item::Definition::associated_function(
        format!("{GRID}::new"),
        new.finish(),
    )?)?;
    crate::length_builders(builder, GRID, &["width", "height"])?;
    crate::pixels_builders(builder, GRID, "spacing")?;
    crate::pixels_builders(builder, GRID, "min_size")?;
    for (name, event) in [("on_click", "Pane"), ("on_drag", "DragEvent")] {
        generic(
            builder,
            format!("{GRID}::{name}"),
            receiver::Mode::Owned,
            [type_ref::Type::callable(ty(event)?, message()?)],
            of_message(GRID)?,
        )?;
    }
    for leeway in crate::PIXELS {
        generic(
            builder,
            format!("{GRID}::on_resize"),
            receiver::Mode::Owned,
            [
                type_ref::Type::builtin(leeway),
                type_ref::Type::callable(ty("ResizeEvent")?, message()?),
            ],
            of_message(GRID)?,
        )?;
    }
    Ok(())
}

/// `Content`, `TitleBar` and `Controls`.
fn declare_parts(builder: &mut facade::Builder) -> Result<(), host::Error> {
    constructor(builder, &format!("{CONTENT}::new"), 1, CONTENT)?;
    constructor(builder, &format!("{TITLE_BAR}::new"), 1, TITLE_BAR)?;
    constructor(builder, &format!("{CONTROLS}::new"), 1, CONTROLS)?;
    constructor(builder, &format!("{CONTROLS}::dynamic"), 2, CONTROLS)?;
    crate::builders(builder, CONTENT, "title_bar", [of_message(TITLE_BAR)?])?;
    // iced's `impl Into<Controls>`: `Controls`, or an element.
    crate::builders(
        builder,
        TITLE_BAR,
        "controls",
        [of_message(CONTROLS)?, element()?],
    )?;
    crate::padding_builders(builder, TITLE_BAR)?;
    generic(
        builder,
        format!("{TITLE_BAR}::always_show_controls"),
        receiver::Mode::Owned,
        [],
        of_message(TITLE_BAR)?,
    )?;
    for owner in [CONTENT, TITLE_BAR] {
        crate::builders(
            builder,
            owner,
            "style",
            [
                type_ref::Type::registered("widget::container::Builtin")?,
                // iced's `impl Fn(&Theme) -> container::Style`.
                type_ref::Type::callable(
                    type_ref::Type::registered("Theme")?,
                    type_ref::Type::registered("widget::container::Style")?,
                ),
            ],
        )?;
    }
    Ok(())
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        message.into().into(),
        Position::NONE,
    ))
}

fn home_error(error: home::Error) -> Box<EvalAltResult> {
    self::error(error.to_string())
}

fn handle(value: &Dynamic, operation: &str) -> Result<home::Handle, Box<EvalAltResult>> {
    value
        .read_lock::<State>()
        .map(|state| state.native.clone())
        .ok_or_else(|| error(format!("{operation} expects a `pane_grid::State`")))
}

fn copied<T: Copy + 'static>(value: &Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>> {
    value.read_lock::<T>().map(|value| *value).ok_or_else(|| {
        error(format!(
            "expected `{expected}`, found `{}`",
            value.type_name()
        ))
    })
}

fn pane(value: &Dynamic) -> Result<native::Pane, Box<EvalAltResult>> {
    copied(value, "pane_grid::Pane")
}

fn usize_value(value: usize) -> Dynamic {
    Dynamic::from(rawr::INT::try_from(value).unwrap_or(rawr::INT::MAX))
}

fn option(value: Option<Dynamic>) -> Dynamic {
    value.map_or_else(rawr::option::none, rawr::option::some)
}

fn style(value: &mut Dynamic) -> Result<marker::container::Source, Box<EvalAltResult>> {
    Ok(if value.is::<marker::container::Style>() {
        marker::container::Source::Builtin(take(value, "container style")?)
    } else {
        marker::container::Source::Function(take(value, "container style function")?)
    })
}

/// Bind the playground's `pane_grid`.
pub(crate) fn bind(builder: &mut host::runtime::Builder) {
    bind_values(builder);
    bind_state(builder);
    bind_grid(builder);
    bind_parts(builder);
}

fn bind_values(builder: &mut host::runtime::Builder) {
    let paths = AXES
        .map(|(name, axis)| (format!("Axis::{name}"), Dynamic::from(axis)))
        .into_iter()
        .chain(
            DIRECTIONS
                .map(|(name, direction)| (format!("Direction::{name}"), Dynamic::from(direction))),
        )
        .chain(EDGES.map(|(name, edge)| (format!("Edge::{name}"), Dynamic::from(edge))))
        .chain([(
            "Region::Center".to_owned(),
            Dynamic::from(native::Region::Center),
        )]);
    for (path, value) in paths {
        builder
            .bind(
                format!("iced::{MODULE}::{path}"),
                move |_| Ok(value.clone()),
            )
            .expect("the playground's facade must retain every pane_grid value");
    }
    type Read = fn(&Dynamic) -> Result<Dynamic, Box<EvalAltResult>>;
    let readers: [(&str, Read); 7] = [
        ("DragEvent::Picked", |value| {
            Ok(Dynamic::from(matches!(
                copied::<native::DragEvent>(value, "pane_grid::DragEvent")?,
                native::DragEvent::Picked { .. }
            )))
        }),
        ("DragEvent::Dropped", |value| {
            Ok(Dynamic::from(matches!(
                copied::<native::DragEvent>(value, "pane_grid::DragEvent")?,
                native::DragEvent::Dropped { .. }
            )))
        }),
        ("DragEvent::Canceled", |value| {
            Ok(Dynamic::from(matches!(
                copied::<native::DragEvent>(value, "pane_grid::DragEvent")?,
                native::DragEvent::Canceled { .. }
            )))
        }),
        ("DragEvent::pane", |value| {
            Ok(Dynamic::from(
                match copied::<native::DragEvent>(value, "pane_grid::DragEvent")? {
                    native::DragEvent::Picked { pane }
                    | native::DragEvent::Dropped { pane, .. }
                    | native::DragEvent::Canceled { pane } => pane,
                },
            ))
        }),
        ("DragEvent::target", |value| {
            match copied::<native::DragEvent>(value, "pane_grid::DragEvent")? {
                native::DragEvent::Dropped { target, .. } => Ok(Dynamic::from(target)),
                _ => Err(error("only `DragEvent::Dropped` has a `target`")),
            }
        }),
        ("ResizeEvent::split", |value| {
            Ok(Dynamic::from(
                copied::<native::ResizeEvent>(value, "pane_grid::ResizeEvent")?.split,
            ))
        }),
        ("ResizeEvent::ratio", |value| {
            Ok(Dynamic::from(f64::from(
                copied::<native::ResizeEvent>(value, "pane_grid::ResizeEvent")?.ratio,
            )))
        }),
    ];
    for (path, read) in readers {
        let path = format!("iced::{MODULE}::{path}");
        let operation = path.clone();
        builder
            .bind(path, move |args| {
                let [value] = args else {
                    return Err(arity(&operation, 1, args.len()));
                };
                read(value)
            })
            .expect("the playground's facade must retain every pane_grid reader");
    }
}

/// A State method: `operation` names it, `args[0]` is the receiver.
fn bind_method(
    builder: &mut host::runtime::Builder,
    name: &'static str,
    count: usize,
    call: impl Fn(&home::Handle, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
        + Send
        + Sync
        + 'static,
) {
    let operation = format!("pane_grid::State::{name}");
    builder
        .bind(format!("iced::{STATE}::{name}"), move |args| {
            if args.len() != count + 1 {
                return Err(arity(&operation, count + 1, args.len()));
            }
            let (receiver, rest) = args.split_at_mut(1);
            call(&handle(receiver[0], &operation)?, rest)
        })
        .expect("the playground's facade must retain every pane_grid::State method");
}

/// Read the State `handle` names.
fn read<R>(
    handle: &home::Handle,
    f: impl FnOnce(&NativeState) -> R,
) -> Result<R, Box<EvalAltResult>> {
    home::get(handle, f).map_err(home_error)
}

/// Change the State `handle` names in place.
fn change<R>(
    handle: &home::Handle,
    f: impl FnOnce(&mut NativeState) -> R,
) -> Result<R, Box<EvalAltResult>> {
    home::with_mut(handle, f).map_err(home_error)
}

fn bind_state(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation(format!("iced::{STATE}"), |value, expected| {
            let value = value.read_lock::<State>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the playground's facade must declare generic pane_grid::State once");
    builder
        .bind_with_context(format!("iced::{STATE}::new"), |call, args| {
            let [first] = args else {
                return Err(arity("pane_grid::State::new", 1, args.len()));
            };
            let (state, pane) = NativeState::new(first.take());
            let native = home::insert(state).map_err(home_error)?;
            let witness = call
                .output_witness()
                .map_err(witness_error)?
                .argument_witness(0)
                .ok_or_else(|| error("pane_grid::State::new gives a `(State<T>, Pane)`"))?;
            Ok(Dynamic::from(rawr::Array::from([
                Dynamic::from(State { witness, native }),
                Dynamic::from(pane),
            ])))
        })
        .expect("the playground's facade must retain pane_grid::State::new");
    bind_method(builder, "len", 0, |state, _| {
        read(state, |state| usize_value(state.len()))
    });
    bind_method(builder, "is_empty", 0, |state, _| {
        read(state, |state| Dynamic::from(state.is_empty()))
    });
    bind_method(builder, "get", 1, |state, args| {
        let pane = pane(args[0])?;
        read(state, |state| option(state.get(pane).cloned()))
    });
    bind_method(builder, "get_mut", 1, |state, args| {
        let pane = pane(args[0])?;
        if !read(state, |state| state.get(pane).is_some())? {
            return Ok(rawr::option::none());
        }
        Ok(rawr::option::some(Dynamic::from(place(
            state.clone(),
            pane,
        ))))
    });
    bind_method(builder, "adjacent", 2, |state, args| {
        let pane = pane(args[0])?;
        let direction = copied::<native::Direction>(args[1], "pane_grid::Direction")?;
        read(state, |state| {
            option(state.adjacent(pane, direction).map(Dynamic::from))
        })
    });
    bind_method(builder, "maximized", 0, |state, _| {
        read(state, |state| option(state.maximized().map(Dynamic::from)))
    });
    bind_method(builder, "split", 3, |state, args| {
        let axis = copied::<native::Axis>(args[0], "pane_grid::Axis")?;
        let pane = pane(args[1])?;
        let value = args[2].take();
        change(state, |state| {
            option(state.split(axis, pane, value).map(|(pane, split)| {
                Dynamic::from(rawr::Array::from([
                    Dynamic::from(pane),
                    Dynamic::from(split),
                ]))
            }))
        })
    });
    bind_method(builder, "close", 1, |state, args| {
        let pane = pane(args[0])?;
        change(state, |state| {
            option(state.close(pane).map(|(value, sibling)| {
                Dynamic::from(rawr::Array::from([value, Dynamic::from(sibling)]))
            }))
        })
    });
    bind_method(builder, "drop", 2, |state, args| {
        let pane = pane(args[0])?;
        let target = copied::<native::Target>(args[1], "pane_grid::Target")?;
        change(state, |state| state.drop(pane, target))?;
        Ok(Dynamic::UNIT)
    });
    bind_method(builder, "swap", 2, |state, args| {
        let (a, b) = (pane(args[0])?, pane(args[1])?);
        change(state, |state| state.swap(a, b))?;
        Ok(Dynamic::UNIT)
    });
    bind_method(builder, "move_to_edge", 2, |state, args| {
        let pane = pane(args[0])?;
        let edge = copied::<native::Edge>(args[1], "pane_grid::Edge")?;
        change(state, |state| state.move_to_edge(pane, edge))?;
        Ok(Dynamic::UNIT)
    });
    bind_method(builder, "resize", 2, |state, args| {
        let split = copied::<native::Split>(args[0], "pane_grid::Split")?;
        #[allow(clippy::cast_possible_truncation)]
        let ratio = number(args[1], "pane_grid::State::resize")? as f32;
        change(state, |state| state.resize(split, ratio))?;
        Ok(Dynamic::UNIT)
    });
    bind_method(builder, "maximize", 1, |state, args| {
        let pane = pane(args[0])?;
        change(state, |state| state.maximize(pane))?;
        Ok(Dynamic::UNIT)
    });
    bind_method(builder, "restore", 0, |state, _| {
        change(state, NativeState::restore)?;
        Ok(Dynamic::UNIT)
    });
}

/// `get_mut(pane)`'s place: each read copies the pane's value out of the
/// State, and each store puts one back, so a pattern's `&mut` binding
/// writes into the State as Rust's `Option<&mut T>` does. Script code an
/// update runs sees the State unborrowed, so it runs between the read and
/// the store.
fn place(state: home::Handle, pane: native::Pane) -> rawr::place::Place {
    use rawr::place::Access;
    let missing = || error("the pane `get_mut` lent is no longer in the `pane_grid::State`");
    rawr::place::Place::new(move |access| {
        let read = || read(&state, |panes| panes.get(pane).cloned())?.ok_or_else(missing);
        let store = |value: Dynamic| {
            change(&state, |panes| {
                panes.get_mut(pane).map(|target| *target = value).is_some()
            })?
            .then_some(Dynamic::UNIT)
            .ok_or_else(missing)
        };
        match access {
            Access::Read => read(),
            Access::Store(value) => store(value),
            Access::Update(apply) => store(apply(read()?)?),
        }
    })
}

impl Witnessed for Grid {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn take_grid(
    args: &mut [&mut Dynamic],
    operation: &str,
    count: usize,
) -> Result<Grid, Box<EvalAltResult>> {
    if args.len() != count + 1 {
        return Err(arity(operation, count + 1, args.len()));
    }
    take::<Grid>(args[0], "iced::widget::PaneGrid")
}

fn bind_grid(builder: &mut host::runtime::Builder) {
    bind_widget_type::<Grid>(builder, "iced::widget::PaneGrid");
    builder
        .bind_with_context(format!("iced::{GRID}::new"), |call, args| {
            let [state, view] = args else {
                return Err(arity("PaneGrid::new", 2, args.len()));
            };
            Ok(Dynamic::from(Grid {
                witness: call.output_witness().map_err(witness_error)?,
                identity: widget::Identity::unique(),
                state: handle(state, "PaneGrid::new")?,
                view: take::<rawr::item::Value>(view, "symbolic pane view")?,
                width: iced::Length::Fill,
                height: iced::Length::Fill,
                spacing: 0.0,
                min_size: None,
                on_click: None,
                on_drag: None,
                on_resize: None,
            }))
        })
        .expect("the playground's facade must retain PaneGrid::new");
    type Set = fn(&mut Grid, &mut Dynamic) -> Result<(), Box<EvalAltResult>>;
    let setters: [(&str, Set); 6] = [
        ("width", |grid, value| {
            grid.width = length(value)?;
            Ok(())
        }),
        ("height", |grid, value| {
            grid.height = length(value)?;
            Ok(())
        }),
        ("spacing", |grid, value| {
            grid.spacing = pixels(value, "PaneGrid::spacing")?;
            Ok(())
        }),
        ("min_size", |grid, value| {
            grid.min_size = Some(pixels(value, "PaneGrid::min_size")?);
            Ok(())
        }),
        ("on_click", |grid, value| {
            grid.on_click = Some(take(value, "symbolic message mapper")?);
            Ok(())
        }),
        ("on_drag", |grid, value| {
            grid.on_drag = Some(take(value, "symbolic message mapper")?);
            Ok(())
        }),
    ];
    for (name, set) in setters {
        let operation = format!("PaneGrid::{name}");
        builder
            .bind(format!("iced::{GRID}::{name}"), move |args| {
                let mut grid = take_grid(args, &operation, 1)?;
                set(&mut grid, args[1])?;
                Ok(Dynamic::from(grid))
            })
            .expect("the playground's facade must retain every PaneGrid builder");
    }
    builder
        .bind(format!("iced::{GRID}::on_resize"), |args| {
            let mut grid = take_grid(args, "PaneGrid::on_resize", 2)?;
            let leeway = pixels(args[1], "PaneGrid::on_resize")?;
            grid.on_resize = Some((leeway, take(args[2], "symbolic message mapper")?));
            Ok(Dynamic::from(grid))
        })
        .expect("the playground's facade must retain PaneGrid::on_resize");
}

/// The attestation of a `Message`-generic part, as `Content<Message>`.
fn attest<T: Witnessed + Clone + Send + Sync + 'static>(
    builder: &mut host::runtime::Builder,
    path: &str,
) {
    builder
        .bind_type_attestation(format!("iced::{path}"), |value, expected| {
            let value = value.read_lock::<T>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(value.witness())))
                .flatten()
        })
        .expect("the playground's facade must declare each pane_grid part once");
}

impl Witnessed for Content {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for TitleBar {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl Witnessed for Controls {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn bind_parts(builder: &mut host::runtime::Builder) {
    attest::<Content>(builder, CONTENT);
    attest::<TitleBar>(builder, TITLE_BAR);
    attest::<Controls>(builder, CONTROLS);
    builder
        .bind_with_context(format!("iced::{CONTENT}::new"), |call, args| {
            let [body] = args else {
                return Err(arity("pane_grid::Content::new", 1, args.len()));
            };
            Ok(Dynamic::from(Content {
                witness: call.output_witness().map_err(witness_error)?,
                body: take::<widget::Value>(body, "iced::Element")?,
                title_bar: None,
                style: None,
            }))
        })
        .expect("the playground's facade must retain pane_grid::Content::new");
    builder
        .bind_with_context(format!("iced::{TITLE_BAR}::new"), |call, args| {
            let [content] = args else {
                return Err(arity("pane_grid::TitleBar::new", 1, args.len()));
            };
            Ok(Dynamic::from(TitleBar {
                witness: call.output_witness().map_err(witness_error)?,
                content: take::<widget::Value>(content, "iced::Element")?,
                controls: None,
                padding: None,
                always_show_controls: false,
                style: None,
            }))
        })
        .expect("the playground's facade must retain pane_grid::TitleBar::new");
    for (name, count) in [("new", 1), ("dynamic", 2)] {
        let operation = format!("pane_grid::Controls::{name}");
        builder
            .bind_with_context(format!("iced::{CONTROLS}::{name}"), move |call, args| {
                if args.len() != count {
                    return Err(arity(&operation, count, args.len()));
                }
                let full = take::<widget::Value>(args[0], "iced::Element")?;
                let compact = match args.get_mut(1) {
                    Some(compact) => Some(take::<widget::Value>(compact, "iced::Element")?),
                    None => None,
                };
                Ok(Dynamic::from(Controls {
                    witness: call.output_witness().map_err(witness_error)?,
                    full,
                    compact,
                }))
            })
            .expect("the playground's facade must retain pane_grid::Controls");
    }
    builder
        .bind(format!("iced::{CONTENT}::title_bar"), |args| {
            let [content, bar] = args else {
                return Err(arity("pane_grid::Content::title_bar", 2, args.len()));
            };
            let mut content = take::<Content>(content, "iced::widget::pane_grid::Content")?;
            content.title_bar = Some(take::<TitleBar>(bar, "iced::widget::pane_grid::TitleBar")?);
            Ok(Dynamic::from(content))
        })
        .expect("the playground's facade must retain pane_grid::Content::title_bar");
    builder
        .bind(format!("iced::{CONTENT}::style"), |args| {
            let [content, value] = args else {
                return Err(arity("pane_grid::Content::style", 2, args.len()));
            };
            let mut content = take::<Content>(content, "iced::widget::pane_grid::Content")?;
            content.style = Some(style(value)?);
            Ok(Dynamic::from(content))
        })
        .expect("the playground's facade must retain pane_grid::Content::style");
    type Set = fn(&mut TitleBar, &mut Dynamic) -> Result<(), Box<EvalAltResult>>;
    let setters: [(&str, Set); 3] = [
        ("controls", |bar, value| {
            bar.controls = Some(if value.is::<Controls>() {
                take::<Controls>(value, "iced::widget::pane_grid::Controls")?
            } else {
                let full = take::<widget::Value>(value, "iced::Element")?;
                Controls {
                    witness: bar.witness.clone(),
                    full,
                    compact: None,
                }
            });
            Ok(())
        }),
        ("padding", |bar, value| {
            bar.padding = Some(padding(value)?);
            Ok(())
        }),
        ("style", |bar, value| {
            bar.style = Some(style(value)?);
            Ok(())
        }),
    ];
    for (name, set) in setters {
        let operation = format!("pane_grid::TitleBar::{name}");
        builder
            .bind(format!("iced::{TITLE_BAR}::{name}"), move |args| {
                if args.len() != 2 {
                    return Err(arity(&operation, 2, args.len()));
                }
                let mut bar = take::<TitleBar>(args[0], "iced::widget::pane_grid::TitleBar")?;
                set(&mut bar, args[1])?;
                Ok(Dynamic::from(bar))
            })
            .expect("the playground's facade must retain every pane_grid::TitleBar builder");
    }
    builder
        .bind(format!("iced::{TITLE_BAR}::always_show_controls"), |args| {
            let [bar] = args else {
                return Err(arity(
                    "pane_grid::TitleBar::always_show_controls",
                    1,
                    args.len(),
                ));
            };
            let mut bar = take::<TitleBar>(bar, "iced::widget::pane_grid::TitleBar")?;
            bar.always_show_controls = true;
            Ok(Dynamic::from(bar))
        })
        .expect("the playground's facade must retain pane_grid::TitleBar::always_show_controls");
}

/// A container style function for a pane or a title bar, or the
/// diagnostic its failure shows.
fn themed(
    source: &Option<marker::container::Source>,
    context: &widget::Context<'_>,
    widget: &str,
) -> Result<Option<impl Fn(&iced::Theme) -> iced::widget::container::Style + use<>>, Element<'static>>
{
    Ok(container_style(source, context, widget)?
        .map(|style| move |theme: &iced::Theme| style(theme)))
}

impl TitleBar {
    fn lower<'a>(
        &self,
        context: &widget::Context<'a>,
    ) -> Result<native::TitleBar<'a, runtime::Message, Element<'a>>, Element<'static>> {
        let mut bar = native::TitleBar::new(self.content.lower(context));
        if let Some(controls) = &self.controls {
            bar = bar.controls(match &controls.compact {
                None => native::Controls::new(controls.full.lower(context)),
                Some(compact) => {
                    native::Controls::dynamic(controls.full.lower(context), compact.lower(context))
                }
            });
        }
        if let Some(padding) = self.padding {
            bar = bar.padding(padding);
        }
        if self.always_show_controls {
            bar = bar.always_show_controls();
        }
        if let Some(style) = themed(&self.style, context, "pane_grid::TitleBar")? {
            bar = bar.style(style);
        }
        Ok(bar)
    }
}

/// A pane's content as iced takes it: with a title bar, whose content is
/// an element, or with none.
enum Lowered<'a> {
    Titled(native::Content<'a, runtime::Message, Element<'a>, Element<'a>>),
    Untitled(native::Content<'a, runtime::Message, iced::advanced::widget::Void, Element<'a>>),
}

impl Content {
    fn lower<'a>(&self, context: &widget::Context<'a>) -> Result<Lowered<'a>, Element<'static>> {
        let style = themed(&self.style, context, "pane_grid::Content")?;
        let body = self.body.lower(context);
        Ok(match &self.title_bar {
            Some(bar) => {
                let mut content = native::Content::new(body).title_bar(bar.lower(context)?);
                if let Some(style) = style {
                    content = content.style(style);
                }
                Lowered::Titled(content)
            }
            None => {
                let mut content = native::Content::new(body);
                if let Some(style) = style {
                    content = content.style(style);
                }
                Lowered::Untitled(content)
            }
        })
    }
}

impl Grid {
    /// iced's grid over `state`, with the content each pane's view gave.
    fn build<'a, T>(
        &self,
        state: &'a NativeState,
        contents: HashMap<native::Pane, native::Content<'a, runtime::Message, T, Element<'a>>>,
        context: &widget::Context<'a>,
    ) -> Element<'a>
    where
        T: iced::advanced::Widget<runtime::Message, iced::Theme, iced::Renderer> + 'a,
    {
        let contents = RefCell::new(contents);
        let mut grid = native::PaneGrid::new(state, |pane, _, _| {
            contents
                .borrow_mut()
                .remove(&pane)
                .expect("each pane's view ran before the grid")
        })
        .width(self.width)
        .height(self.height)
        .spacing(self.spacing);
        if let Some(min_size) = self.min_size {
            grid = grid.min_size(min_size);
        }
        if let Some(mapper) = &self.on_click {
            let mapper = context.message_mapper(mapper.clone());
            grid = grid.on_click(move |pane| mapper.map(Dynamic::from(pane)));
        }
        if let Some(mapper) = &self.on_drag {
            let mapper = context.message_mapper(mapper.clone());
            grid = grid.on_drag(move |event| mapper.map(Dynamic::from(event)));
        }
        if let Some((leeway, mapper)) = &self.on_resize {
            let mapper = context.message_mapper(mapper.clone());
            grid = grid.on_resize(*leeway, move |event| mapper.map(Dynamic::from(event)));
        }
        grid.boxed()
    }
}

impl widget::Descriptor for Grid {
    fn lower<'a>(&self, context: &widget::Context<'a>) -> Element<'a> {
        let Some(home) = context.home() else {
            return widget::failure(widget::refuse_borrow("PaneGrid", "pane_grid::State"));
        };
        let state = match home.get::<NativeState>(&self.state) {
            Ok(state) => state,
            Err(error) => return widget::failure(format!("PaneGrid: {error}")),
        };
        // iced's `PaneGrid::new` calls the view for each pane; the view is
        // the script's, so each pane's content is made first.
        let maximized = state.maximized();
        let mut titled = HashMap::new();
        let mut untitled = HashMap::new();
        for (pane, value) in state.iter() {
            let arguments = &mut [
                Dynamic::from(*pane),
                value.clone(),
                Dynamic::from(maximized == Some(*pane)),
            ];
            let content = match context.invoke(&self.view, arguments) {
                None => return iced::widget::space().boxed(),
                Some(Ok(content)) => content,
                Some(Err(error)) => {
                    return widget::failure(format!("pane_grid view failed: {error}"));
                }
            };
            let Some(content) = content
                .read_lock::<Content>()
                .map(|content| content.clone())
            else {
                return widget::failure(format!(
                    "pane_grid view gave `{}`, expected `pane_grid::Content`",
                    content.type_name()
                ));
            };
            match content.lower(context) {
                Ok(Lowered::Titled(content)) => {
                    titled.insert(*pane, content);
                }
                Ok(Lowered::Untitled(content)) => {
                    untitled.insert(*pane, content);
                }
                Err(failure) => return failure,
            }
        }
        match (titled.is_empty(), untitled.is_empty()) {
            (_, true) => self.build(state, titled, context),
            (true, false) => self.build(state, untitled, context),
            (false, false) => widget::failure(
                "pane_grid view gave some panes a title bar and others none; iced's `PaneGrid` takes one kind",
            ),
        }
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}
