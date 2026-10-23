//! Exact runtime bindings for the closed scripted-widget carrier algebra.

use rawr::host;
use rawr::{Dynamic, EvalAltResult, Position};

use crate::widget;

pub(crate) fn bind(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    bind_with_renderer(iced, rawr, fill_quad);
}

pub(crate) fn bind_profile(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    bind_with_renderer(iced, rawr, fill_quad);
}

fn bind_with_renderer(
    iced: &mut host::runtime::Builder,
    rawr: &mut host::runtime::Builder,
    renderer: impl Fn(&host::runtime::Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
        + Send
        + Sync
        + 'static,
) {
    bind_attestations(rawr);
    bind_canonical(iced, rawr);
    bind_children(rawr);
    bind_layout(rawr);
    bind_event(rawr);
    bind_draw(rawr, renderer);
    bind_mouse(rawr);
    bind_operation(rawr);
    bind_overlay(rawr);
}

fn bind_attestations(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation("rawr_iced::widget::children::List", |value, expected| {
            let list = value.read_lock::<widget::children::List>()?;
            if expected.argument_count() != 1
                || !list
                    .values()
                    .iter()
                    .all(|child| expected.arguments_match_witness(child.witness()))
            {
                return None;
            }
            expected.attest_witness(Some(list.witness()))
        })
        .expect("the rawr_iced facade must declare generic widget::children::List exactly once");
    builder
        .bind_type_attestation("rawr_iced::widget::event::Outcome", |value, expected| {
            let outcome = value.read_lock::<widget::event::Outcome>()?;
            if expected.argument_count() != 1
                || !outcome
                    .message()
                    .is_none_or(|message| expected.argument_matches(0, message))
            {
                return None;
            }
            expected.attest_witness(Some(outcome.witness()))
        })
        .expect("the rawr_iced facade must declare generic widget::event::Outcome exactly once");
    builder
        .bind_type_attestation("rawr_iced::widget::overlay::Value", |value, expected| {
            let value = value.read_lock::<widget::overlay::Value>()?;
            if expected.argument_count() != 1
                || value
                    .content()
                    .is_some_and(|content| !expected.arguments_match_witness(content.witness()))
            {
                return None;
            }
            expected.attest_witness(Some(value.witness()))
        })
        .expect("the rawr_iced facade must declare generic widget::overlay::Value exactly once");
}

fn bind_canonical(iced: &mut host::runtime::Builder, rawr: &mut host::runtime::Builder) {
    let builder = iced;
    for (name, value) in [
        ("BLACK", iced::Color::BLACK),
        ("TRANSPARENT", iced::Color::TRANSPARENT),
        ("WHITE", iced::Color::WHITE),
    ] {
        builder
            .bind(format!("iced::Color::{name}"), move |args| {
                expect_arity(name, args, 0)?;
                Ok(Dynamic::from(value))
            })
            .expect("the iced facade must retain every canonical Color value");
    }
    builder
        .bind("iced::Color::from_rgb", |args| {
            expect_arity("Color::from_rgb", args, 3)?;
            Ok(Dynamic::from(iced::Color::from_rgb(
                unit(args[0], "Color::from_rgb red")?,
                unit(args[1], "Color::from_rgb green")?,
                unit(args[2], "Color::from_rgb blue")?,
            )))
        })
        .expect("the iced facade must retain Color::from_rgb");
    builder
        .bind("iced::Color::from_rgba", |args| {
            expect_arity("Color::from_rgba", args, 4)?;
            Ok(Dynamic::from(iced::Color::from_rgba(
                unit(args[0], "Color::from_rgba red")?,
                unit(args[1], "Color::from_rgba green")?,
                unit(args[2], "Color::from_rgba blue")?,
                unit(args[3], "Color::from_rgba alpha")?,
            )))
        })
        .expect("the iced facade must retain Color::from_rgba");
    builder
        .bind("iced::Color::from_rgb8", |args| {
            expect_arity("Color::from_rgb8", args, 3)?;
            Ok(Dynamic::from(iced::Color::from_rgb8(
                byte(args[0], "Color::from_rgb8 red")?,
                byte(args[1], "Color::from_rgb8 green")?,
                byte(args[2], "Color::from_rgb8 blue")?,
            )))
        })
        .expect("the iced facade must retain Color::from_rgb8");
    builder
        .bind("iced::Color::from_rgba8", |args| {
            expect_arity("Color::from_rgba8", args, 4)?;
            Ok(Dynamic::from(iced::Color::from_rgba8(
                byte(args[0], "Color::from_rgba8 red")?,
                byte(args[1], "Color::from_rgba8 green")?,
                byte(args[2], "Color::from_rgba8 blue")?,
                unit(args[3], "Color::from_rgba8 alpha")?,
            )))
        })
        .expect("the iced facade must retain Color::from_rgba8");
    builder
        .bind("iced::Color::from_packed_rgb8", |args| {
            expect_arity("Color::from_packed_rgb8", args, 1)?;
            Ok(Dynamic::from(iced::Color::from_packed_rgb8(packed(
                args[0],
                "Color::from_packed_rgb8",
            )?)))
        })
        .expect("the iced facade must retain Color::from_packed_rgb8");
    builder
        .bind("iced::Color::from_packed_rgba8", |args| {
            expect_arity("Color::from_packed_rgba8", args, 2)?;
            Ok(Dynamic::from(iced::Color::from_packed_rgba8(
                packed(args[0], "Color::from_packed_rgba8")?,
                unit(args[1], "Color::from_packed_rgba8 alpha")?,
            )))
        })
        .expect("the iced facade must retain Color::from_packed_rgba8");

    for (name, value) in [
        ("Fit", iced::Length::Fit),
        ("Fill", iced::Length::Fill),
        ("Shrink", iced::Length::Shrink),
    ] {
        for path in [format!("iced::Length::{name}"), format!("iced::{name}")] {
            builder
                .bind(path, move |args| {
                    expect_arity(name, args, 0)?;
                    Ok(Dynamic::from(value))
                })
                .expect("the iced facade must retain every canonical Length value");
        }
    }
    builder
        .bind("iced::Length::Fixed", |args| {
            expect_arity("Length::Fixed", args, 1)?;
            Ok(Dynamic::from(iced::Length::Fixed(pixels(
                args[0],
                "Length::Fixed",
            )?)))
        })
        .expect("the iced facade must retain Length::Fixed");
    for (path, bound) in [
        (
            "iced::Length::min",
            (|length: iced::Length, pixels: f32| length.min(pixels))
                as fn(iced::Length, f32) -> iced::Length,
        ),
        ("iced::Length::max", |length, pixels| length.max(pixels)),
    ] {
        builder
            .bind(path, move |args| {
                expect_arity(path, args, 2)?;
                let length = take::<iced::Length>(args[0], "iced::Length")?;
                Ok(Dynamic::from(bound(length, pixels(args[1], path)?)))
            })
            .expect("the iced facade must retain every Length bound");
    }
    for path in ["iced::Length::FillPortion", "iced::FillPortion"] {
        builder
            .bind(path, |args| {
                expect_arity("Length::FillPortion", args, 1)?;
                Ok(Dynamic::from(iced::Length::FillPortion(portion(
                    args[0],
                    "Length::FillPortion",
                )?)))
            })
            .expect("the iced facade must retain Length::FillPortion");
    }
    builder
        .bind("iced::Padding::ZERO", |args| {
            expect_arity("Padding::ZERO", args, 0)?;
            Ok(Dynamic::from(iced::Padding::ZERO))
        })
        .expect("the iced facade must retain Padding::ZERO");
    builder
        .bind("iced::Padding::new", |args| {
            expect_arity("Padding::new", args, 1)?;
            Ok(Dynamic::from(iced::Padding::new(pixels(
                args[0],
                "Padding::new",
            )?)))
        })
        .expect("the iced facade must retain Padding::new");
    for (side, set) in PADDING_SIDES {
        builder
            .bind(format!("iced::padding::{side}"), move |args| {
                expect_arity(side, args, 1)?;
                Ok(Dynamic::from(set(iced::Padding::ZERO, pixels(args[0], side)?)))
            })
            .expect("the iced facade must retain every padding function");
        if side != "all" {
            builder
                .bind(format!("iced::Padding::{side}"), move |args| {
                    expect_arity(side, args, 2)?;
                    let padding = take::<iced::Padding>(args[0], "iced::Padding")?;
                    Ok(Dynamic::from(set(padding, pixels(args[1], side)?)))
                })
                .expect("the iced facade must retain every Padding side method");
        }
    }
    for (name, font) in [
        ("DEFAULT", iced::Font::DEFAULT),
        ("MONOSPACE", iced::Font::MONOSPACE),
    ] {
        builder
            .bind(format!("iced::Font::{name}"), move |args| {
                expect_arity(name, args, 0)?;
                Ok(Dynamic::from(font))
            })
            .expect("the iced facade must retain every canonical Font value");
    }
    builder
        .bind("iced::Point::new", |args| {
            expect_arity("Point::new", args, 2)?;
            Ok(Dynamic::from(iced::Point::new(
                finite(args[0], "Point::new x")?,
                finite(args[1], "Point::new y")?,
            )))
        })
        .expect("the iced facade must retain Point::new");
    builder
        .bind("iced::Vector::new", |args| {
            expect_arity("Vector::new", args, 2)?;
            Ok(Dynamic::from(iced::Vector::new(
                finite(args[0], "Vector::new x")?,
                finite(args[1], "Vector::new y")?,
            )))
        })
        .expect("the iced facade must retain Vector::new");
    builder
        .bind("iced::Rectangle::new", |args| {
            expect_arity("Rectangle::new", args, 2)?;
            let origin = take::<iced::Point>(args[0], "iced::Point")?;
            let size = take::<iced::Size>(args[1], "iced::Size")?;
            Ok(Dynamic::from(iced::Rectangle::new(origin, size)))
        })
        .expect("the iced facade must retain Rectangle::new");
    builder
        .bind("iced::Size::new", |args| {
            expect_arity("Size::new", args, 2)?;
            Ok(Dynamic::from(iced::Size::new(
                nonnegative(args[0], "Size::new width")?,
                nonnegative(args[1], "Size::new height")?,
            )))
        })
        .expect("the iced facade must retain Size::new");
    rawr.bind("rawr_iced::widget::layout::Size::new", |args| {
        expect_arity("widget::layout::Size::new", args, 2)?;
        Ok(Dynamic::from(iced::Size::new(
            take::<iced::Length>(args[0], "iced::Length")?,
            take::<iced::Length>(args[1], "iced::Length")?,
        )))
    })
    .expect("the rawr_iced facade must retain layout::Size::new");
    rawr.bind("rawr_iced::widget::layout::Size::fixed", |args| {
        expect_arity("Size::fixed", args, 2)?;
        Ok(Dynamic::from(iced::Size::new(
            iced::Length::Fixed(pixels(args[0], "Size::fixed width")?),
            iced::Length::Fixed(pixels(args[1], "Size::fixed height")?),
        )))
    })
    .expect("the rawr_iced facade must retain layout::Size::fixed");

    for (name, value) in interactions() {
        builder
            .bind(format!("iced::mouse::Interaction::{name}"), move |args| {
                expect_arity(name, args, 0)?;
                Ok(Dynamic::from(value))
            })
            .expect("the iced facade must retain every canonical mouse interaction");
    }
}

fn bind_children(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("rawr_iced::widget::children::list", |call, args| {
            let values = args
                .iter_mut()
                .map(|value| take::<widget::Value>(value, "iced::Element"))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Dynamic::from(widget::children::List::new(
                call.output_witness().map_err(witness_error)?,
                values,
            )))
        })
        .expect("the rawr_iced facade must retain widget::children::list");
}

fn bind_layout(builder: &mut host::runtime::Builder) {
    builder
        .bind("rawr_iced::widget::layout::Input::resolve", |args| {
            expect_arity("layout::Input::resolve", args, 4)?;
            let input = take::<widget::layout::Input>(args[0], "rawr_iced::widget::layout::Input")?;
            let requested = take::<iced::Size<iced::Length>>(args[1], "iced::Size")?;
            let intrinsic = iced::Size::new(
                pixels(args[2], "layout intrinsic width")?,
                pixels(args[3], "layout intrinsic height")?,
            );
            Ok(Dynamic::from(input.resolve(requested, intrinsic)))
        })
        .expect("the rawr_iced facade must retain layout::Input::resolve");
    builder
        .bind("rawr_iced::widget::layout::Node::child", |args| {
            expect_arity("layout::Node::child", args, 6)?;
            let node = take::<widget::layout::Node>(args[0], "rawr_iced::widget::layout::Node")?;
            let index = index(args[1], "layout child index")?;
            let bounds = rectangle(args[2], args[3], args[4], args[5], "layout child bounds")?;
            if node.children().iter().any(|child| child.index() == index) {
                return Err(runtime_error(format!(
                    "layout child index {index} is positioned more than once"
                )));
            }
            Ok(Dynamic::from(node.child(index, bounds)))
        })
        .expect("the rawr_iced facade must retain layout::Node::child");
}

fn bind_event(builder: &mut host::runtime::Builder) {
    builder
        .bind("rawr_iced::widget::event::Input::is_press", |args| {
            expect_arity("event::Input::is_press", args, 1)?;
            let input = take::<widget::event::Input>(args[0], "rawr_iced::widget::event::Input")?;
            Ok(Dynamic::from(input.is_press()))
        })
        .expect("the rawr_iced facade must retain event::Input::is_press");
    builder
        .bind_with_context("rawr_iced::widget::event::ignore", |call, args| {
            expect_arity("event::ignore", args, 0)?;
            Ok(Dynamic::from(widget::event::Outcome::new(
                call.output_witness().map_err(witness_error)?,
                iced::event::Status::Ignored,
                None,
            )))
        })
        .expect("the rawr_iced facade must retain event::ignore");
    builder
        .bind_with_context("rawr_iced::widget::event::publish", |call, args| {
            expect_arity("event::publish", args, 1)?;
            Ok(Dynamic::from(widget::event::Outcome::new(
                call.output_witness().map_err(witness_error)?,
                iced::event::Status::Captured,
                Some(args[0].take()),
            )))
        })
        .expect("the rawr_iced facade must retain event::publish");
    builder
        .bind("rawr_iced::widget::event::Outcome::when", |args| {
            expect_arity("event::Outcome::when", args, 2)?;
            let outcome =
                take::<widget::event::Outcome>(args[0], "rawr_iced::widget::event::Outcome")?;
            let enabled = args[1].as_bool().map_err(|_| {
                runtime_error(format!(
                    "event::Outcome::when expects `bool`, found `{}`",
                    args[1].type_name()
                ))
            })?;
            Ok(Dynamic::from(outcome.when(enabled)))
        })
        .expect("the rawr_iced facade must retain event::Outcome::when");
}

fn bind_draw(
    builder: &mut host::runtime::Builder,
    renderer: impl Fn(&host::runtime::Call<'_>, &mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>
        + Send
        + Sync
        + 'static,
) {
    builder
        .bind_with_context("rawr_iced::Renderer::fill_quad", renderer)
        .expect("the rawr_iced facade must retain Renderer::fill_quad");
    builder
        .bind_with_context("rawr_iced::Renderer::fill_rounded_quad", fill_rounded_quad)
        .expect("the rawr_iced facade must retain Renderer::fill_rounded_quad");

    for (name, output) in [
        ("bounds", DrawOutput::Rectangle),
        ("viewport", DrawOutput::Rectangle),
        ("text_color", DrawOutput::Color),
    ] {
        builder
            .bind(
                format!("rawr_iced::widget::draw::Input::{name}"),
                move |args| {
                    expect_arity(name, args, 1)?;
                    let input =
                        take::<widget::draw::Input>(args[0], "rawr_iced::widget::draw::Input")?;
                    Ok(match output {
                        DrawOutput::Rectangle if name == "bounds" => Dynamic::from(input.bounds()),
                        DrawOutput::Rectangle => Dynamic::from(input.viewport()),
                        DrawOutput::Color => Dynamic::from(input.text_color()),
                    })
                },
            )
            .expect("the rawr_iced facade must retain every bounded draw input accessor");
    }
}

fn fill_quad(
    call: &host::runtime::Call<'_>,
    args: &mut [&mut Dynamic],
) -> Result<Dynamic, Box<EvalAltResult>> {
    expect_arity("Renderer::fill_quad", args, 3)?;
    let expected = call
        .invocation::<crate::renderer::Context>()
        .map_err(witness_error)?;
    let bounds = take::<iced::Rectangle>(args[1], "iced::Rectangle")?;
    let color = take::<iced::Color>(args[2], "iced::Color")?;
    let lease = args[0]
        .read_lock::<crate::renderer::Lease>()
        .ok_or_else(|| runtime_error("expected invocation-scoped rawr_iced::Renderer".into()))?;
    lease
        .fill_quad(expected, bounds, color)
        .map_err(|error| runtime_error(error.to_string()))?;
    Ok(Dynamic::UNIT)
}

fn fill_rounded_quad(
    call: &host::runtime::Call<'_>,
    args: &mut [&mut Dynamic],
) -> Result<Dynamic, Box<EvalAltResult>> {
    expect_arity("Renderer::fill_rounded_quad", args, 4)?;
    let expected = call
        .invocation::<crate::renderer::Context>()
        .map_err(witness_error)?;
    let bounds = take::<iced::Rectangle>(args[1], "iced::Rectangle")?;
    let color = take::<iced::Color>(args[2], "iced::Color")?;
    let radius = pixels(args[3], "rounded quad radius")?;
    let lease = args[0]
        .read_lock::<crate::renderer::Lease>()
        .ok_or_else(|| runtime_error("expected invocation-scoped rawr_iced::Renderer".into()))?;
    lease
        .fill_rounded_quad(expected, bounds, color, radius)
        .map_err(|error| runtime_error(error.to_string()))?;
    Ok(Dynamic::UNIT)
}

#[derive(Clone, Copy)]
enum DrawOutput {
    Rectangle,
    Color,
}

fn bind_mouse(builder: &mut host::runtime::Builder) {
    builder
        .bind("rawr_iced::widget::mouse::Input::is_over", |args| {
            expect_arity("mouse::Input::is_over", args, 1)?;
            let input = take::<widget::mouse::Input>(args[0], "rawr_iced::widget::mouse::Input")?;
            Ok(Dynamic::from(input.is_over()))
        })
        .expect("the rawr_iced facade must retain mouse::Input::is_over");
}

fn bind_operation(builder: &mut host::runtime::Builder) {
    builder
        .bind("rawr_iced::widget::operation::Input::bounds", |args| {
            expect_arity("operation::Input::bounds", args, 1)?;
            let input =
                take::<widget::operation::Input>(args[0], "rawr_iced::widget::operation::Input")?;
            Ok(Dynamic::from(input.bounds()))
        })
        .expect("the rawr_iced facade must retain operation::Input::bounds");
    for (name, outcome) in [
        ("skip", widget::operation::Outcome::SKIP),
        ("container", widget::operation::Outcome::CONTAINER),
        ("traverse", widget::operation::Outcome::TRAVERSE),
    ] {
        builder
            .bind(
                format!("rawr_iced::widget::operation::{name}"),
                move |args| {
                    expect_arity(name, args, 0)?;
                    Ok(Dynamic::from(outcome))
                },
            )
            .expect("the rawr_iced facade must retain every bounded operation outcome");
    }
}

fn bind_overlay(builder: &mut host::runtime::Builder) {
    builder
        .bind("rawr_iced::widget::overlay::Input::viewport", |args| {
            expect_arity("overlay::Input::viewport", args, 1)?;
            let input =
                take::<widget::overlay::Input>(args[0], "rawr_iced::widget::overlay::Input")?;
            Ok(Dynamic::from(input.viewport()))
        })
        .expect("the rawr_iced facade must retain overlay::Input::viewport");
    builder
        .bind("rawr_iced::widget::overlay::Input::translation", |args| {
            expect_arity("overlay::Input::translation", args, 1)?;
            let input =
                take::<widget::overlay::Input>(args[0], "rawr_iced::widget::overlay::Input")?;
            Ok(Dynamic::from(input.translation()))
        })
        .expect("the rawr_iced facade must retain overlay::Input::translation");
    builder
        .bind_with_context("rawr_iced::widget::overlay::none", |call, args| {
            expect_arity("overlay::none", args, 0)?;
            Ok(Dynamic::from(widget::overlay::Value::new(
                call.output_witness().map_err(witness_error)?,
                None,
            )))
        })
        .expect("the rawr_iced facade must retain overlay::none");
    builder
        .bind_with_context("rawr_iced::widget::overlay::some", |call, args| {
            expect_arity("overlay::some", args, 1)?;
            let content = take::<widget::Value>(args[0], "iced::Element")?;
            Ok(Dynamic::from(widget::overlay::Value::new(
                call.output_witness().map_err(witness_error)?,
                Some(content),
            )))
        })
        .expect("the rawr_iced facade must retain overlay::some");
}

fn interactions() -> [(&'static str, iced::mouse::Interaction); 27] {
    use iced::mouse::Interaction;

    [
        ("None", Interaction::None),
        ("Hidden", Interaction::Hidden),
        ("Idle", Interaction::Idle),
        ("ContextMenu", Interaction::ContextMenu),
        ("Help", Interaction::Help),
        ("Pointer", Interaction::Pointer),
        ("Progress", Interaction::Progress),
        ("Wait", Interaction::Wait),
        ("Cell", Interaction::Cell),
        ("Crosshair", Interaction::Crosshair),
        ("Text", Interaction::Text),
        ("Alias", Interaction::Alias),
        ("Copy", Interaction::Copy),
        ("Move", Interaction::Move),
        ("NoDrop", Interaction::NoDrop),
        ("NotAllowed", Interaction::NotAllowed),
        ("Grab", Interaction::Grab),
        ("Grabbing", Interaction::Grabbing),
        ("ResizingHorizontally", Interaction::ResizingHorizontally),
        ("ResizingVertically", Interaction::ResizingVertically),
        ("ResizingDiagonallyUp", Interaction::ResizingDiagonallyUp),
        (
            "ResizingDiagonallyDown",
            Interaction::ResizingDiagonallyDown,
        ),
        ("ResizingColumn", Interaction::ResizingColumn),
        ("ResizingRow", Interaction::ResizingRow),
        ("AllScroll", Interaction::AllScroll),
        ("ZoomIn", Interaction::ZoomIn),
        ("ZoomOut", Interaction::ZoomOut),
    ]
}

fn rectangle(
    x: &Dynamic,
    y: &Dynamic,
    width: &Dynamic,
    height: &Dynamic,
    operation: &str,
) -> Result<iced::Rectangle, Box<EvalAltResult>> {
    Ok(iced::Rectangle::new(
        iced::Point::new(coordinate(x, operation)?, coordinate(y, operation)?),
        iced::Size::new(pixels(width, operation)?, pixels(height, operation)?),
    ))
}

fn index(value: &Dynamic, operation: &str) -> Result<usize, Box<EvalAltResult>> {
    let value = value.as_int().map_err(|_| {
        runtime_error(format!(
            "{operation} expects non-negative `i64`, found `{}`",
            value.type_name()
        ))
    })?;
    usize::try_from(value).map_err(|_| {
        runtime_error(format!(
            "{operation} expects non-negative `i64`, found {value}"
        ))
    })
}

fn coordinate(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    value.as_int().map(|value| value as f32).map_err(|_| {
        runtime_error(format!(
            "{operation} expects `i64` coordinates, found `{}`",
            value.type_name()
        ))
    })
}

/// Non-negative pixels from an integer or a float, as iced's `impl Into<Pixels>`.
fn pixels(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    let pixels = match (value.as_int(), value.as_float()) {
        (Ok(value), _) => value as f32,
        (_, Ok(value)) => value as f32,
        _ => {
            return Err(runtime_error(format!(
                "{operation} expects pixels as `i64` or `f64`, found `{}`",
                value.type_name()
            )))
        }
    };
    if !pixels.is_finite() || pixels < 0.0 {
        return Err(runtime_error(format!(
            "{operation} expects non-negative pixels, found {pixels}"
        )));
    }
    Ok(pixels)
}

type PaddingSide = fn(iced::Padding, f32) -> iced::Padding;

const PADDING_SIDES: [(&str, PaddingSide); 7] = [
    ("all", |_, value| iced::Padding::new(value)),
    ("top", |padding, value| padding.top(value)),
    ("bottom", |padding, value| padding.bottom(value)),
    ("left", |padding, value| padding.left(value)),
    ("right", |padding, value| padding.right(value)),
    ("horizontal", |padding, value| padding.horizontal(value)),
    ("vertical", |padding, value| padding.vertical(value)),
];

fn finite(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    let value = value.as_float().map_err(|_| {
        runtime_error(format!(
            "{operation} expects `f64`, found `{}`",
            value.type_name()
        ))
    })?;
    let value = value as f32;
    if !value.is_finite() {
        return Err(runtime_error(format!("{operation} expects a finite value")));
    }
    Ok(value)
}

fn nonnegative(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    let value = finite(value, operation)?;
    if value < 0.0 {
        return Err(runtime_error(format!(
            "{operation} expects a non-negative value, found {value}"
        )));
    }
    Ok(value)
}

fn unit(value: &Dynamic, operation: &str) -> Result<f32, Box<EvalAltResult>> {
    let value = finite(value, operation)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(runtime_error(format!(
            "{operation} expects a value in 0.0..=1.0, found {value}"
        )));
    }
    Ok(value)
}

/// A `u8` color channel.
fn byte(value: &Dynamic, operation: &str) -> Result<u8, Box<EvalAltResult>> {
    let value = value.as_int().map_err(|_| {
        runtime_error(format!(
            "{operation} expects `i64`, found `{}`",
            value.type_name()
        ))
    })?;
    u8::try_from(value)
        .map_err(|_| runtime_error(format!("{operation} expects 0..=255, found {value}")))
}

/// A packed `0xrrggbb` color.
fn packed(value: &Dynamic, operation: &str) -> Result<u32, Box<EvalAltResult>> {
    let value = value.as_int().map_err(|_| {
        runtime_error(format!(
            "{operation} expects `i64`, found `{}`",
            value.type_name()
        ))
    })?;
    u32::try_from(value)
        .ok()
        .filter(|value| *value <= 0xff_ffff)
        .ok_or_else(|| runtime_error(format!("{operation} expects 0..=0xffffff, found {value}")))
}

fn portion(value: &Dynamic, operation: &str) -> Result<u16, Box<EvalAltResult>> {
    let value = value.as_int().map_err(|_| {
        runtime_error(format!(
            "{operation} expects a positive `i64`, found `{}`",
            value.type_name()
        ))
    })?;
    let portion = u16::try_from(value).map_err(|_| {
        runtime_error(format!(
            "{operation} expects a positive value no greater than {}, found {value}",
            u16::MAX
        ))
    })?;
    if portion == 0 {
        return Err(runtime_error(format!(
            "{operation} expects a positive value, found 0"
        )));
    }
    Ok(portion)
}

fn take<T>(value: &mut Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>>
where
    T: Clone + 'static,
{
    let actual = value.type_name();
    value.take().try_cast_result::<T>().map_err(|_| {
        runtime_error(format!(
            "scripted widget carrier received `{actual}`, expected `{expected}`"
        ))
    })
}

fn expect_arity(
    operation: &str,
    args: &[&mut Dynamic],
    expected: usize,
) -> Result<(), Box<EvalAltResult>> {
    if args.len() == expected {
        return Ok(());
    }
    Err(runtime_error(format!(
        "scripted widget binding `{operation}` received {} operand(s), expected {expected}",
        args.len()
    )))
}

fn witness_error(error: host::runtime::WitnessError) -> Box<EvalAltResult> {
    runtime_error(error.to_string())
}

fn runtime_error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, OnceLock};

    use rawr::{application, host, resolve, Dynamic, Engine, ParseErrorType, Shared};

    use crate::widget;

    #[test]
    fn overlay_attestation_rejects_unattested_wrong_and_foreign_content() {
        let unattested = widget::Value::new(Stub {
            identity: widget::Identity::unique(),
        });
        assert_overlay_content_is_rejected(unattested);

        let foreign = evaluate(
            "use iced::Element;",
            "Element<Message>",
            "text(\"foreign\")",
        )
        .try_cast_result::<widget::Value>()
        .unwrap();
        assert!(foreign.witness().is_some());
        assert_overlay_content_is_rejected(foreign);

        let wrong_message = evaluate(
            r#"
                use iced::Element;
                #[derive(Debug, Clone, Copy)]
                enum Other { Tick }
            "#,
            "Element<Other>",
            "text(\"wrong message\")",
        )
        .try_cast_result::<widget::Value>()
        .unwrap();
        assert!(wrong_message.witness().is_some());
        assert_overlay_content_is_rejected(wrong_message);
    }

    #[test]
    fn source_operations_mint_exact_closed_carriers() {
        let size = evaluate(
            "use iced::Length; use rawr_iced::widget::layout::Size;",
            "Size",
            "Size::new(Length::Shrink, Length::Fill)",
        )
        .try_cast_result::<iced::Size<iced::Length>>()
        .unwrap();
        assert_eq!(
            size,
            iced::Size::new(iced::Length::Shrink, iced::Length::Fill)
        );
        let fixed = evaluate(
            "use rawr_iced::widget::layout::Size;",
            "Size",
            "Size::fixed(40, 24)",
        )
        .try_cast_result::<iced::Size<iced::Length>>()
        .unwrap();
        assert_eq!(fixed.width, iced::Length::Fixed(40.0));
        assert_eq!(fixed.height, iced::Length::Fixed(24.0));

        let color = evaluate(
            "use iced::Color;",
            "Color",
            "Color::from_rgba(0.25, 0.5, 0.75, 1.0)",
        )
        .try_cast_result::<iced::Color>()
        .unwrap();
        assert_eq!(color, iced::Color::from_rgba(0.25, 0.5, 0.75, 1.0));

        let point = evaluate("use iced::Point;", "Point", "Point::new(-2.5, 4.0)")
            .try_cast_result::<iced::Point>()
            .unwrap();
        assert_eq!(point, iced::Point::new(-2.5, 4.0));

        let rectangle = evaluate(
            "use iced::{Point, Rectangle, Size};",
            "Rectangle",
            "Rectangle::new(Point::new(1.0, 2.0), Size::new(40.0, 24.0))",
        )
        .try_cast_result::<iced::Rectangle>()
        .unwrap();
        assert_eq!(
            rectangle,
            iced::Rectangle::new(iced::Point::new(1.0, 2.0), iced::Size::new(40.0, 24.0))
        );

        let children = evaluate(
            "use rawr_iced::widget::children::List; use rawr_iced::widget::children::list;",
            "List<Message>",
            "list![]",
        )
        .try_cast_result::<widget::children::List>()
        .unwrap();
        assert!(children.values().is_empty());

        let event = evaluate(
            "use rawr_iced::widget::event::Outcome; use rawr_iced::widget::event::publish;",
            "Outcome<Message>",
            "publish(Message::Tick)",
        )
        .try_cast_result::<widget::event::Outcome>()
        .unwrap();
        assert_eq!(event.status(), iced::event::Status::Captured);
        assert!(event.message().is_some());
        let witness = event.witness().clone();
        assert_eq!(event.clone().when(false).witness(), &witness);
        assert_eq!(event.clone().when(true).witness(), &witness);

        let suppressed = evaluate(
            "use rawr_iced::widget::event::Outcome; use rawr_iced::widget::event::publish;",
            "Outcome<Message>",
            "publish(Message::Tick).when(false)",
        )
        .try_cast_result::<widget::event::Outcome>()
        .unwrap();
        assert_eq!(suppressed.status(), iced::event::Status::Ignored);
        assert!(suppressed.message().is_none());

        let retained = evaluate(
            "use rawr_iced::widget::event::Outcome; use rawr_iced::widget::event::publish;",
            "Outcome<Message>",
            "publish(Message::Tick).when(true)",
        )
        .try_cast_result::<widget::event::Outcome>()
        .unwrap();
        assert_eq!(retained.status(), iced::event::Status::Captured);
        assert!(retained.message().is_some());

        let mouse = evaluate(
            "use iced::mouse::Interaction;",
            "Interaction",
            "Interaction::Pointer",
        )
        .try_cast_result::<iced::mouse::Interaction>()
        .unwrap();
        assert_eq!(mouse, iced::mouse::Interaction::Pointer);

        let operation = evaluate(
            "use rawr_iced::widget::operation::Outcome; use rawr_iced::widget::operation::container;",
            "Outcome",
            "container()",
        )
        .try_cast_result::<widget::operation::Outcome>()
        .unwrap();
        assert!(operation.is_container());
        assert!(!operation.should_traverse());

        let overlay = evaluate(
            "use rawr_iced::widget::overlay::Value; use rawr_iced::widget::overlay::none;",
            "Value<Message>",
            "none()",
        )
        .try_cast_result::<widget::overlay::Value>()
        .unwrap();
        assert!(overlay.content().is_none());

        let overlay = evaluate(
            "use rawr_iced::widget::overlay::{Value, some};",
            "Value<Message>",
            "some(text(\"overlay\"))",
        )
        .try_cast_result::<widget::overlay::Value>()
        .unwrap();
        assert!(overlay.content().is_some());
    }

    #[test]
    fn integer_and_string_keys_round_trip_through_the_exact_overloads() {
        let integer = evaluate_key("key(value(), 42)");
        assert!(matches!(
            integer.key(),
            Some(widget::key::Value::Integer(42))
        ));
        assert!(integer.witness().is_some());

        let string = evaluate_key("key(value(), \"forty-two\")");
        assert!(matches!(
            string.key(),
            Some(widget::key::Value::String(value)) if value == "forty-two"
        ));

        let mapped = evaluate_key("key(value(), 42).map(lift)");
        assert!(matches!(
            mapped.key(),
            Some(widget::key::Value::Integer(42))
        ));
        assert!(string.witness().is_some());

        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let error = engine
            .compile_application(key_application("key(value(), true)"))
            .unwrap_err();
        assert!(matches!(
            error.err_type(),
            ParseErrorType::Resolution(resolve::Error::InvalidHostOperation {
                item,
                reason,
                ..
            }) if item == "rawr_iced::widget::key" && reason.contains("no form of it takes")
        ), "{error:?}");
        assert!(!error.primary_span().is_none());
    }

    #[test]
    fn adapter_inputs_are_owned_bounded_facts() {
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(10.0, 10.0),
            iced::Size::new(100.0, 100.0),
        );
        let node = widget::layout::Input::new(limits)
            .resolve(
                iced::Size::new(iced::Length::Fill, iced::Length::Fill),
                iced::Size::new(20.0, 30.0),
            )
            .child(
                0,
                iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(10.0, 10.0)),
            );
        assert_eq!(node.size(), iced::Size::new(100.0, 100.0));
        assert_eq!(node.children()[0].bounds().width, 10.0);

        assert!(widget::event::Input::new(true).is_press());
        assert!(widget::mouse::Input::new(true).is_over());
        assert_eq!(
            widget::operation::Input::new(iced::Rectangle::with_size(iced::Size::new(2.0, 3.0)))
                .bounds()
                .height,
            3.0
        );
        let overlay = widget::overlay::Input::new(
            iced::Rectangle::with_size(iced::Size::new(5.0, 6.0)),
            iced::Vector::new(1.0, 2.0),
        );
        assert_eq!(overlay.viewport().height, 6.0);
        assert_eq!(overlay.translation().x, 1.0);

        let draw = widget::draw::Input::new(
            iced::Rectangle::with_size(iced::Size::new(7.0, 8.0)),
            Some(iced::Point::new(1.0, 2.0)),
            iced::Rectangle::with_size(iced::Size::new(9.0, 10.0)),
            iced::Color::WHITE,
        );
        assert_eq!(draw.bounds().width, 7.0);
        assert_eq!(draw.cursor(), Some(iced::Point::new(1.0, 2.0)));
        assert_eq!(draw.viewport().height, 10.0);
        assert_eq!(draw.text_color(), iced::Color::WHITE);
    }

    #[test]
    fn nested_evaluation_keeps_each_registry_call_context_exact() {
        let harness = Arc::new(Harness::default());
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(draw_application())
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        for facade in nested_runtime(harness.clone()) {
            registry.install(facade).unwrap();
        }
        let launch = launch.bind(registry).unwrap();
        let data = launch.boot_default().unwrap();
        let descriptor = launch
            .promote_widget(&launch.widget_promotions()[0], data)
            .unwrap();
        let binding = launch.bind_widget(&descriptor).unwrap();
        let promotion = binding.promotion();
        let instance = application::Instance::new();
        let outer = crate::renderer::Transaction::bind(
            instance,
            binding.generation(),
            promotion.site().clone(),
            promotion.definition().fingerprint().clone(),
        )
        .unwrap();
        let nested = crate::renderer::Transaction::bind(
            instance,
            binding.generation(),
            promotion.site().clone(),
            promotion.definition().fingerprint().clone(),
        )
        .unwrap();
        assert!(harness.binding.set(binding.clone()).is_ok());
        assert!(harness.lease.set(nested.lease()).is_ok());
        assert!(harness.invocation.set(nested.invocation().clone()).is_ok());

        assert_eq!(
            binding
                .draw(
                    None,
                    Dynamic::from(outer.lease()),
                    Dynamic::from(draw_input()),
                    outer.invocation(),
                )
                .unwrap(),
            Some(())
        );
        assert!(harness.cross_rejected.load(Ordering::SeqCst));
        assert_eq!(harness.phase.load(Ordering::SeqCst), 2);
        assert_eq!(outer.commit().unwrap().len(), 2);
        assert_eq!(nested.commit().unwrap().len(), 2);
    }

    #[derive(Default)]
    struct Harness {
        binding: OnceLock<rawr::widget::Binding>,
        lease: OnceLock<crate::renderer::Lease>,
        invocation: OnceLock<host::runtime::invocation::Invocation<crate::renderer::Context>>,
        phase: AtomicUsize,
        cross_rejected: AtomicBool,
    }

    fn nested_runtime(harness: Arc<Harness>) -> [host::runtime::Facade; 2] {
        let iced = crate::facade().unwrap();
        let rawr = crate::extension(&iced).unwrap();
        let mut iced = host::runtime::Builder::new(iced);
        let mut rawr = host::runtime::Builder::new(rawr);
        crate::task::bind(&mut iced, &mut rawr);
        crate::theme::bind(&mut iced, &mut rawr);
        widget::builtin::bind(&mut iced, &mut rawr);
        widget::script::bind(&mut iced);
        super::bind_with_renderer(
            &mut iced,
            &mut rawr,
            move |call, args| {
                let lease = args[0]
                    .read_lock::<crate::renderer::Lease>()
                    .ok_or_else(|| {
                        super::runtime_error("expected rawr_iced Renderer lease".into())
                    })?
                    .clone();
                let result = super::fill_quad(call, args)?;

                if harness
                    .phase
                    .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    let binding = harness.binding.get().ok_or_else(|| {
                        super::runtime_error("missing nested widget binding".into())
                    })?;
                    let invocation = harness.invocation.get().ok_or_else(|| {
                        super::runtime_error("missing nested renderer invocation".into())
                    })?;
                    let cross = binding.draw(
                        None,
                        Dynamic::from(lease),
                        Dynamic::from(draw_input()),
                        invocation,
                    );
                    harness
                        .cross_rejected
                        .store(cross.is_err(), Ordering::SeqCst);

                    let nested = binding
                        .draw(
                            None,
                            Dynamic::from(
                                harness
                                    .lease
                                    .get()
                                    .ok_or_else(|| {
                                        super::runtime_error("missing nested renderer lease".into())
                                    })?
                                    .clone(),
                            ),
                            Dynamic::from(draw_input()),
                            invocation,
                        )
                        .map_err(|error| super::runtime_error(error.to_string()))?;
                    if nested != Some(()) {
                        return Err(super::runtime_error(
                            "nested widget omitted its retained draw hook".into(),
                        ));
                    }
                    harness.phase.store(2, Ordering::SeqCst);
                }

                Ok(result)
            },
        );
        [iced.finish(), rawr.finish()]
    }

    fn draw_input() -> widget::draw::Input {
        widget::draw::Input::new(
            iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(8.0, 8.0)),
            None,
            iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(8.0, 8.0)),
            iced::Color::BLACK,
        )
    }

    fn draw_application() -> &'static str {
        r#"
            use iced::{Color, Result};
            use iced::Element;

            #[derive(Default)]
            struct Dial { radius: i64 }
            #[derive(Debug, Clone, Copy)]
            enum Message { Tick }

            #[widget]
            impl Dial {
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
                    renderer.fill_quad(input.bounds(), Color::BLACK);
                    renderer.fill_quad(input.viewport(), Color::WHITE);
                }
            }

            impl Dial {
                fn update(&mut self, message: Message) {
                    match message { Message::Tick => () }
                }

                fn view(&self) -> Element<Message> { Dial { radius: self.radius } }
            }

            pub fn main() -> Result { iced::run(Dial::update, Dial::view) }
        "#
    }

    fn evaluate(imports: &str, output: &str, body: &str) -> Dynamic {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let candidate = engine
            .compile_application(application(imports, output, body))
            .unwrap();
        let items = candidate.items().cloned().collect::<Vec<_>>();
        let descriptor = candidate.validate().unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = Arc::new(launch.bind(registry).unwrap());
        let item = items
            .iter()
            .find(|item| item.key().name() == "make")
            .unwrap();
        launch.invoke(item, &mut []).unwrap()
    }

    fn evaluate_key(body: &str) -> widget::Value {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let candidate = engine.compile_application(key_application(body)).unwrap();
        let items = candidate.items().cloned().collect::<Vec<_>>();
        let descriptor = candidate.validate().unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = Arc::new(launch.bind(registry).unwrap());
        let item = items
            .iter()
            .find(|item| item.key().name() == "make")
            .unwrap();
        launch
            .invoke(item, &mut [])
            .unwrap()
            .try_cast_result::<widget::Value>()
            .unwrap()
    }

    fn key_application(body: &str) -> String {
        format!(
            r#"
            use iced::Element;
            use iced::widget::{{Text, text}};
            use rawr_iced::widget::key;

            #[derive(Default)]
            struct State;

            #[derive(Debug, Clone, Copy)]
            enum Message {{ Tick }}

            pub fn value() -> Element<Message> {{ text("key carrier") }}
            pub fn lift(message: Message) -> Message {{ message }}
            pub fn make() -> Element<Message> {{ {body} }}

            impl State {{
                fn update(&mut self, _message: Message) {{}}
                fn view(&self) -> Text {{ text("key carrier") }}
            }}

            pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
        "#
        )
    }

    fn assert_overlay_content_is_rejected(content: widget::Value) {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let candidate = engine
            .compile_application(application(
                "use rawr_iced::widget::overlay::{Value, none};",
                "Value<Message>",
                "none()",
            ))
            .unwrap();
        let items = candidate.items().cloned().collect::<Vec<_>>();
        let descriptor = candidate.validate().unwrap();
        let launch = engine.launch_application(descriptor).unwrap();

        let iced = crate::facade().unwrap();
        let rawr = crate::extension(&iced).unwrap();
        let mut iced = host::runtime::Builder::new(iced);
        let mut rawr = host::runtime::Builder::new(rawr);
        crate::task::bind(&mut iced, &mut rawr);
        crate::theme::bind(&mut iced, &mut rawr);
        widget::builtin::bind(&mut iced, &mut rawr);
        widget::script::bind(&mut iced);
        super::bind_attestations(&mut rawr);
        super::bind_canonical(&mut iced, &mut rawr);
        rawr.bind_with_context("rawr_iced::widget::overlay::none", move |call, args| {
            super::expect_arity("overlay::none", args, 0)?;
            Ok(Dynamic::from(widget::overlay::Value::new(
                call.output_witness().map_err(super::witness_error)?,
                Some(content.clone()),
            )))
        })
        .unwrap();
        let mut registry = host::runtime::Registry::new();
        registry.install(iced.finish()).unwrap();
        registry.install(rawr.finish()).unwrap();
        let launch = Arc::new(launch.bind(registry).unwrap());
        let item = items
            .iter()
            .find(|item| item.key().name() == "make")
            .unwrap();

        assert!(launch.invoke(item, &mut []).is_err());
    }

    struct Stub {
        identity: widget::Identity,
    }

    impl widget::Descriptor for Stub {
        fn lower(
            &self,
            _context: &widget::Context<'_>,
        ) -> iced::Element<'static, crate::runtime::Message> {
            iced::widget::Widget::boxed(iced::widget::text("unattested"))
        }

        fn identity(&self) -> widget::Identity {
            self.identity.clone()
        }
    }

    fn application(imports: &str, output: &str, body: &str) -> String {
        format!(
            r#"
            {imports}
            use iced::widget::Text;
            use iced::widget::text;

            #[derive(Default)]
            struct State;

            #[derive(Debug, Clone, Copy)]
            enum Message {{ Tick }}

            pub fn make() -> {output} {{ {body} }}

            impl State {{
                fn update(&mut self, _message: Message) {{}}
                fn view(&self) -> Text {{ text("carrier checkpoint") }}
            }}

            pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
        "#
        )
    }
}
