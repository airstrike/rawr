//! iced's `canvas`: a script `canvas::Program` whose `draw` records its
//! drawing on `Frame`s, which iced's own canvas replays, and whose `update`
//! and `mouse_interaction` reach iced's events.

use std::sync::{Arc, Mutex};

use iced::widget::canvas;
use iced::widget::Widget as _;
use rawr::{host, Dynamic, EvalAltResult};

use super::builtin::{arity, length, pixels, runtime_error, take, witness_error, Witnessed};
use crate::{marker, runtime, theme};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    bind_program(builder);
    bind_paths(builder);
    bind_frames(builder);
    bind_styles(builder);
    bind_bounds(builder);
    #[cfg(feature = "image")]
    if crate::features::on(crate::features::Feature::Image) {
        bind_images(builder);
    }
}

/// `frame.draw_image(bounds, image)`, and `canvas::Image::new(handle)` with
/// its `rotation` and `opacity`.
#[cfg(feature = "image")]
fn bind_images(builder: &mut host::runtime::Builder) {
    use iced::widget::image::Handle;
    /// iced's `impl Into<Image>`: an `Image`, or a `Handle` drawn as it is.
    fn image(value: &Dynamic, path: &str) -> Result<canvas::Image, Box<EvalAltResult>> {
        if let Some(handle) = value.read_lock::<Handle>() {
            return Ok(canvas::Image::new(handle.clone()));
        }
        read::<canvas::Image>(value, path)
    }
    builder
        .bind("iced::widget::canvas::Frame::draw_image", |args| {
            const PATH: &str = "Frame::draw_image";
            let [frame, bounds, drawn] = args else {
                return Err(arity(PATH, 3, args.len()));
            };
            let frame = read::<marker::Frame>(frame, PATH)?;
            let draw = marker::Draw::Image(read(bounds, PATH)?, image(drawn, PATH)?);
            record(&frame, draw, PATH)
        })
        .expect("the iced facade must retain Frame::draw_image");
    builder
        .bind("iced::widget::canvas::Image::new", |args| {
            let [handle] = args else {
                return Err(arity("Image::new", 1, args.len()));
            };
            Ok(Dynamic::from(canvas::Image::new(read::<Handle>(
                handle,
                "Image::new",
            )?)))
        })
        .expect("the iced facade must retain canvas::Image::new");
    for name in ["rotation", "opacity"] {
        let path = format!("iced::widget::canvas::Image::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [receiver, operand] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let image = read::<canvas::Image>(receiver, &path)?;
                Ok(Dynamic::from(match name {
                    // iced's `rotation(impl Into<Radians>)`: radians as a
                    // number, or an angle.
                    "rotation" => image.rotation(match operand.as_float() {
                        Ok(radians) => iced::Radians(radians as f32),
                        Err(_) => super::angle::radians(operand)?,
                    }),
                    _ => image.opacity(pixels(operand, &path)?),
                }))
            })
            .expect("the iced facade must retain every canvas::Image builder");
    }
}

fn bind_program(builder: &mut host::runtime::Builder) {
    builder
        .bind_with_context("iced::widget::canvas::Instance::new", |call, args| {
            let [value, update, draw, interaction, state] = args else {
                return Err(arity("canvas::Instance::new", 5, args.len()));
            };
            Ok(Dynamic::from(marker::CanvasProgram {
                witness: call.output_witness().map_err(witness_error)?,
                value: value.take(),
                update: take(update, "canvas update")?,
                draw: take(draw, "canvas draw")?,
                interaction: take(interaction, "canvas mouse_interaction")?,
                state: state.take(),
            }))
        })
        .expect("the iced facade must retain the canvas Instance item");
    builder
        .bind_type_attestation("iced::widget::canvas::Instance", |value, expected| {
            let value = value.read_lock::<marker::CanvasProgram>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic canvas::Instance exactly once");
    for path in ["iced::widget::canvas", "iced::widget::Canvas::new"] {
        builder
            .bind_with_context(path, move |call, args| {
                let [program] = args else {
                    return Err(arity(path, 1, args.len()));
                };
                Ok(Dynamic::from(marker::Canvas {
                    witness: call.output_witness().map_err(witness_error)?,
                    identity: super::Identity::unique(),
                    program: take(program, "canvas::Program")?,
                    width: iced::Length::Fixed(100.0),
                    height: iced::Length::Fixed(100.0),
                }))
            })
            .expect("the iced facade must retain canvas");
    }
    super::helpers::bind_widget_type::<marker::Canvas>(builder, "iced::widget::Canvas");
    for name in ["width", "height"] {
        let path = format!("iced::widget::Canvas::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut canvas = take::<marker::Canvas>(args[0], "iced::widget::Canvas")?;
                let length = length(args[1])?;
                if name == "width" {
                    canvas.width = length;
                } else {
                    canvas.height = length;
                }
                Ok(Dynamic::from(canvas))
            })
            .expect("the iced facade must retain the Canvas size builders");
    }

    builder
        .bind_type_attestation("iced::widget::canvas::Action", |value, expected| {
            let value = value.read_lock::<marker::CanvasAction>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic canvas::Action exactly once");
    for name in ["publish", "none", "capture", "request_redraw"] {
        let path = format!("iced::widget::canvas::Action::{name}");
        builder
            .bind_with_context(path.clone(), move |call, args| {
                let count = args.len();
                let message = match (name, args) {
                    ("publish", [message]) => Some(message.take()),
                    ("publish", _) => return Err(arity(&path, 1, count)),
                    (_, []) => None,
                    _ => return Err(arity(&path, 0, count)),
                };
                Ok(Dynamic::from(marker::CanvasAction {
                    witness: call.output_witness().map_err(witness_error)?,
                    message,
                    redraw: name == "request_redraw",
                    capture: name == "capture",
                }))
            })
            .expect("the iced facade must retain every canvas::Action constructor");
    }
    builder
        .bind_with_context("iced::widget::canvas::Action::and_capture", |call, args| {
            let [action] = args else {
                return Err(arity("Action::and_capture", 1, args.len()));
            };
            let mut action = take::<marker::CanvasAction>(action, "iced::widget::canvas::Action")?;
            action.witness = call
                .transform_witness(&action.witness)
                .map_err(witness_error)?;
            action.capture = true;
            Ok(Dynamic::from(action))
        })
        .expect("the iced facade must retain canvas::Action::and_capture");
}

/// A value of `T` read through `value`, which stays as it is: a shared
/// receiver, or a copy-free read.
fn read<T: Clone + 'static>(value: &Dynamic, path: &str) -> Result<T, Box<EvalAltResult>> {
    value
        .read_lock::<T>()
        .map(|value| value.clone())
        .ok_or_else(|| {
            runtime_error(format!(
                "`{path}` received `{}`, expected `{}`",
                value.type_name(),
                std::any::type_name::<T>()
            ))
        })
}

fn point(value: &Dynamic, path: &str) -> Result<iced::Point, Box<EvalAltResult>> {
    read::<iced::Point>(value, path)
}

fn size(value: &Dynamic, path: &str) -> Result<iced::Size, Box<EvalAltResult>> {
    read::<iced::Size>(value, path)
}

fn bind_paths(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::widget::canvas::Path::circle", |args| {
            let [center, radius] = args else {
                return Err(arity("Path::circle", 2, args.len()));
            };
            let path = "Path::circle";
            Ok(Dynamic::from(canvas::Path::circle(
                point(center, path)?,
                pixels(radius, path)?,
            )))
        })
        .expect("the iced facade must retain Path::circle");
    builder
        .bind("iced::widget::canvas::Path::line", |args| {
            let [from, to] = args else {
                return Err(arity("Path::line", 2, args.len()));
            };
            let path = "Path::line";
            Ok(Dynamic::from(canvas::Path::line(
                point(from, path)?,
                point(to, path)?,
            )))
        })
        .expect("the iced facade must retain Path::line");
    builder
        .bind("iced::widget::canvas::Path::rectangle", |args| {
            let [top_left, extent] = args else {
                return Err(arity("Path::rectangle", 2, args.len()));
            };
            let path = "Path::rectangle";
            Ok(Dynamic::from(canvas::Path::rectangle(
                point(top_left, path)?,
                size(extent, path)?,
            )))
        })
        .expect("the iced facade must retain Path::rectangle");
    builder
        .bind("iced::widget::canvas::Path::rounded_rectangle", |args| {
            let [top_left, extent, radius] = args else {
                return Err(arity("Path::rounded_rectangle", 3, args.len()));
            };
            let path = "Path::rounded_rectangle";
            Ok(Dynamic::from(canvas::Path::rounded_rectangle(
                point(top_left, path)?,
                size(extent, path)?,
                pixels(radius, path)?.into(),
            )))
        })
        .expect("the iced facade must retain Path::rounded_rectangle");
    builder
        .bind("iced::widget::canvas::path::Builder::new", |args| {
            if !args.is_empty() {
                return Err(arity("path::Builder::new", 0, args.len()));
            }
            Ok(Dynamic::from(marker::PathBuilder(Arc::new(Mutex::new(
                Some(canvas::path::Builder::new()),
            )))))
        })
        .expect("the iced facade must retain path::Builder::new");
    for name in [
        "move_to",
        "line_to",
        "circle",
        "rectangle",
        "rounded_rectangle",
        "arc_to",
        "quadratic_curve_to",
        "bezier_curve_to",
        "close",
    ] {
        let path = format!("iced::widget::canvas::path::Builder::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let handle = read::<marker::PathBuilder>(receiver, &path)?;
                let expected = match name {
                    "close" => 0,
                    "move_to" | "line_to" => 1,
                    "circle" | "rectangle" | "quadratic_curve_to" => 2,
                    _ => 3,
                };
                if operands.len() != expected {
                    return Err(arity(&path, expected + 1, operands.len() + 1));
                }
                let mut guard = handle
                    .0
                    .lock()
                    .map_err(|_| runtime_error(format!("`{path}` found its builder poisoned")))?;
                let Some(builder) = guard.as_mut() else {
                    return Err(runtime_error(format!(
                        "`{path}` drew on a builder whose path was already built"
                    )));
                };
                match name {
                    "move_to" => builder.move_to(point(operands[0], &path)?),
                    "line_to" => builder.line_to(point(operands[0], &path)?),
                    "circle" => {
                        builder.circle(point(operands[0], &path)?, pixels(operands[1], &path)?)
                    }
                    "rectangle" => {
                        builder.rectangle(point(operands[0], &path)?, size(operands[1], &path)?)
                    }
                    "rounded_rectangle" => builder.rounded_rectangle(
                        point(operands[0], &path)?,
                        size(operands[1], &path)?,
                        pixels(operands[2], &path)?.into(),
                    ),
                    "arc_to" => builder.arc_to(
                        point(operands[0], &path)?,
                        point(operands[1], &path)?,
                        pixels(operands[2], &path)?,
                    ),
                    "quadratic_curve_to" => {
                        builder.quadratic_curve_to(
                            point(operands[0], &path)?,
                            point(operands[1], &path)?,
                        );
                    }
                    "bezier_curve_to" => builder.bezier_curve_to(
                        point(operands[0], &path)?,
                        point(operands[1], &path)?,
                        point(operands[2], &path)?,
                    ),
                    _ => builder.close(),
                }
                Ok(Dynamic::UNIT)
            })
            .expect("the iced facade must retain every path::Builder method");
    }
    builder
        .bind("iced::widget::canvas::path::Builder::build", |args| {
            let [receiver] = args else {
                return Err(arity("path::Builder::build", 1, args.len()));
            };
            let handle = read::<marker::PathBuilder>(receiver, "path::Builder::build")?;
            let built = handle
                .0
                .lock()
                .map_err(|_| {
                    runtime_error("`path::Builder::build` found its builder poisoned".into())
                })?
                .take()
                .ok_or_else(|| {
                    runtime_error("`path::Builder::build` built its path twice".into())
                })?;
            Ok(Dynamic::from(built.build()))
        })
        .expect("the iced facade must retain path::Builder::build");
}

/// iced's `impl Into<Fill>`: a `Fill`, or a `Color` filled solid.
fn fill(value: &Dynamic, path: &str) -> Result<canvas::Fill, Box<EvalAltResult>> {
    if let Some(color) = value.read_lock::<iced::Color>() {
        return Ok(canvas::Fill::from(*color));
    }
    read::<canvas::Fill>(value, path)
}

fn record(
    frame: &marker::Frame,
    draw: marker::Draw,
    path: &str,
) -> Result<Dynamic, Box<EvalAltResult>> {
    frame
        .draws
        .lock()
        .map_err(|_| runtime_error(format!("`{path}` found its frame poisoned")))?
        .push(draw);
    Ok(Dynamic::UNIT)
}

fn bind_frames(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::widget::canvas::Frame::new", |args| {
            let [_renderer, extent] = args else {
                return Err(arity("Frame::new", 2, args.len()));
            };
            Ok(Dynamic::from(marker::Frame {
                size: size(extent, "Frame::new")?,
                draws: Arc::default(),
            }))
        })
        .expect("the iced facade must retain Frame::new");
    for name in [
        "width",
        "height",
        "size",
        "center",
        "fill",
        "fill_rectangle",
        "stroke",
        "stroke_rectangle",
        "fill_text",
        "translate",
        "rotate",
        "scale",
        "scale_nonuniform",
        "push_transform",
        "pop_transform",
    ] {
        let path = format!("iced::widget::canvas::Frame::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let frame = read::<marker::Frame>(receiver, &path)?;
                let operand = |index: usize| -> Result<&Dynamic, Box<EvalAltResult>> {
                    operands
                        .get(index)
                        .map(|operand| &**operand)
                        .ok_or_else(|| arity(&path, index + 2, operands.len() + 1))
                };
                let draw = match name {
                    "width" => return Ok(Dynamic::from(f64::from(frame.size.width))),
                    "height" => return Ok(Dynamic::from(f64::from(frame.size.height))),
                    "size" => return Ok(Dynamic::from(frame.size)),
                    "center" => {
                        return Ok(Dynamic::from(iced::Point::new(
                            frame.size.width / 2.0,
                            frame.size.height / 2.0,
                        )))
                    }
                    "fill" => marker::Draw::Fill(
                        read::<canvas::Path>(operand(0)?, &path)?,
                        fill(operand(1)?, &path)?,
                    ),
                    "fill_rectangle" => marker::Draw::FillRectangle(
                        point(operand(0)?, &path)?,
                        size(operand(1)?, &path)?,
                        fill(operand(2)?, &path)?,
                    ),
                    "stroke" => marker::Draw::Stroke(
                        read::<canvas::Path>(operand(0)?, &path)?,
                        read::<marker::Stroke>(operand(1)?, &path)?,
                    ),
                    "stroke_rectangle" => marker::Draw::StrokeRectangle(
                        point(operand(0)?, &path)?,
                        size(operand(1)?, &path)?,
                        read::<marker::Stroke>(operand(2)?, &path)?,
                    ),
                    "fill_text" => {
                        let text = operand(0)?;
                        marker::Draw::FillText(match text.read_lock::<rawr::ImmutableString>() {
                            Some(content) => canvas::Text::from(content.as_str()),
                            None => read::<canvas::Text>(text, &path)?,
                        })
                    }
                    "translate" => {
                        marker::Draw::Translate(read::<iced::Vector>(operand(0)?, &path)?)
                    }
                    // iced's `rotate(impl Into<Radians>)`: radians as a
                    // number, or an angle.
                    "rotate" => marker::Draw::Rotate(match operand(0)?.as_float() {
                        Ok(radians) => radians as f32,
                        Err(_) => super::angle::radians(operand(0)?)?.0,
                    }),
                    "scale" => marker::Draw::Scale(pixels(operand(0)?, &path)?),
                    "scale_nonuniform" => {
                        marker::Draw::ScaleNonuniform(read::<iced::Vector>(operand(0)?, &path)?)
                    }
                    "push_transform" => marker::Draw::PushTransform,
                    _ => marker::Draw::PopTransform,
                };
                record(&frame, draw, &path)
            })
            .expect("the iced facade must retain every Frame method");
    }
    builder
        .bind("iced::widget::canvas::Frame::into_geometry", |args| {
            let [receiver] = args else {
                return Err(arity("Frame::into_geometry", 1, args.len()));
            };
            let frame = read::<marker::Frame>(receiver, "Frame::into_geometry")?;
            let draws = std::mem::take(&mut *frame.draws.lock().map_err(|_| {
                runtime_error("`Frame::into_geometry` found its frame poisoned".into())
            })?);
            Ok(Dynamic::from(marker::Geometry {
                size: frame.size,
                draws: draws.into(),
            }))
        })
        .expect("the iced facade must retain Frame::into_geometry");

    for name in ["new", "default", "with_group"] {
        builder
            .bind(format!("iced::widget::canvas::Cache::{name}"), |_| {
                Ok(Dynamic::from(marker::Cache::default()))
            })
            .expect("the iced facade must retain Cache::new");
    }
    builder
        .bind("iced::widget::canvas::Group::unique", |_| {
            Ok(Dynamic::from(marker::CacheGroup))
        })
        .expect("the iced facade must retain Group::unique");
    builder
        .bind("iced::widget::canvas::Cache::clear", |args| {
            let [receiver] = args else {
                return Err(arity("Cache::clear", 1, args.len()));
            };
            let cache = read::<marker::Cache>(receiver, "Cache::clear")?;
            *cache
                .0
                .lock()
                .map_err(|_| runtime_error("`Cache::clear` found its cache poisoned".into()))? =
                None;
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Cache::clear");
    builder
        .bind("iced::widget::canvas::Cache::draw", |_| {
            Err(runtime_error(
                "`Cache::draw` runs its closure where it is written, so Rawr calls it there".into(),
            ))
        })
        .expect("the iced facade must retain Cache::draw");
    builder
        .bind("iced::widget::canvas::Cache::__rawr_cached", |args| {
            let [receiver, extent] = args else {
                return Err(arity("Cache::draw", 2, args.len()));
            };
            let cache = read::<marker::Cache>(receiver, "Cache::draw")?;
            let extent = size(extent, "Cache::draw")?;
            let cached = cache
                .0
                .lock()
                .map_err(|_| runtime_error("`Cache::draw` found its cache poisoned".into()))?
                .as_ref()
                .filter(|geometry| geometry.size == extent)
                .cloned();
            Ok(match cached {
                Some(geometry) => rawr::option::some(Dynamic::from(geometry)),
                None => rawr::option::none(),
            })
        })
        .expect("the iced facade must retain Cache::draw's read");
    builder
        .bind("iced::widget::canvas::Cache::__rawr_store", |args| {
            let [receiver, geometry] = args else {
                return Err(arity("Cache::draw", 2, args.len()));
            };
            let cache = read::<marker::Cache>(receiver, "Cache::draw")?;
            let geometry = read::<marker::Geometry>(geometry, "Cache::draw")?;
            *cache
                .0
                .lock()
                .map_err(|_| runtime_error("`Cache::draw` found its cache poisoned".into()))? =
                Some(geometry.clone());
            Ok(Dynamic::from(geometry))
        })
        .expect("the iced facade must retain Cache::draw's store");
    for path in [
        "iced::widget::canvas::Path::new",
        "iced::widget::canvas::Frame::with_save",
    ] {
        builder
            .bind(path, move |_| {
                Err(runtime_error(format!(
                    "`{path}` runs its closure where it is written, so Rawr calls it there"
                )))
            })
            .expect("the iced facade must retain every closure-running item");
    }
}

fn bind_styles(builder: &mut host::runtime::Builder) {
    fn value<T: Clone + Send + Sync + 'static>(
        builder: &mut host::runtime::Builder,
        path: &'static str,
        value: T,
    ) {
        builder
            .bind(path, move |_| Ok(Dynamic::from(value.clone())))
            .expect("the iced facade must retain every canvas value");
    }
    value(
        builder,
        "iced::widget::canvas::Stroke::default",
        marker::Stroke::default(),
    );
    value(
        builder,
        "iced::widget::canvas::Fill::default",
        canvas::Fill::default(),
    );
    value(
        builder,
        "iced::widget::canvas::LineDash::default",
        marker::LineDash::default(),
    );
    // A dash pattern's fields, read or set
    // (`LineDash { segments: &[3.0, 6.0], offset: 0 }`).
    for name in ["segments", "offset"] {
        let path = format!("iced::widget::canvas::LineDash::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let mut dash = read::<marker::LineDash>(receiver, &path)?;
                let [operand] = operands else {
                    return Ok(match name {
                        "segments" => Dynamic::from(
                            dash.segments
                                .iter()
                                .map(|segment| Dynamic::from(f64::from(*segment)))
                                .collect::<rawr::Array>(),
                        ),
                        _ => Dynamic::from(dash.offset as rawr::INT),
                    });
                };
                match name {
                    "segments" => {
                        let values = operand
                            .read_lock::<rawr::Array>()
                            .ok_or_else(|| {
                                runtime_error(format!("`{path}` expected a slice of numbers"))
                            })?
                            .iter()
                            .map(|segment| pixels(segment, &path))
                            .collect::<Result<Vec<_>, _>>()?;
                        dash.segments = values.into();
                    }
                    _ => {
                        dash.offset =
                            usize::try_from(operand.as_int().map_err(|_| {
                                runtime_error(format!("`{path}` expected a usize"))
                            })?)
                            .map_err(|_| runtime_error(format!("`{path}` expected a usize")))?;
                    }
                }
                Ok(Dynamic::from(dash))
            })
            .expect("the iced facade must retain every LineDash field");
    }
    value(
        builder,
        "iced::widget::canvas::Text::default",
        canvas::Text::default(),
    );
    value(
        builder,
        "iced::widget::canvas::LineCap::default",
        canvas::LineCap::default(),
    );
    value(
        builder,
        "iced::widget::canvas::LineJoin::default",
        canvas::LineJoin::default(),
    );
    value(
        builder,
        "iced::widget::canvas::LineCap::Butt",
        canvas::LineCap::Butt,
    );
    value(
        builder,
        "iced::widget::canvas::LineCap::Square",
        canvas::LineCap::Square,
    );
    value(
        builder,
        "iced::widget::canvas::LineCap::Round",
        canvas::LineCap::Round,
    );
    value(
        builder,
        "iced::widget::canvas::LineJoin::Miter",
        canvas::LineJoin::Miter,
    );
    value(
        builder,
        "iced::widget::canvas::LineJoin::Round",
        canvas::LineJoin::Round,
    );
    value(
        builder,
        "iced::widget::canvas::LineJoin::Bevel",
        canvas::LineJoin::Bevel,
    );
    value(
        builder,
        "iced::widget::canvas::fill::Rule::NonZero",
        canvas::fill::Rule::NonZero,
    );
    value(
        builder,
        "iced::widget::canvas::fill::Rule::EvenOdd",
        canvas::fill::Rule::EvenOdd,
    );
    value(
        builder,
        "iced::widget::text::Alignment::Default",
        iced::widget::text::Alignment::Default,
    );
    value(
        builder,
        "iced::widget::text::Alignment::Left",
        iced::widget::text::Alignment::Left,
    );
    value(
        builder,
        "iced::widget::text::Alignment::Center",
        iced::widget::text::Alignment::Center,
    );
    value(
        builder,
        "iced::widget::text::Alignment::Right",
        iced::widget::text::Alignment::Right,
    );
    value(
        builder,
        "iced::widget::text::Alignment::Justified",
        iced::widget::text::Alignment::Justified,
    );
    builder
        .bind("iced::widget::canvas::Style::Solid", |args| {
            let [color] = args else {
                return Err(arity("Style::Solid", 1, args.len()));
            };
            Ok(Dynamic::from(canvas::Style::Solid(read::<iced::Color>(
                color,
                "Style::Solid",
            )?)))
        })
        .expect("the iced facade must retain Style::Solid");

    // A stroke's fields, read or set (`Stroke { width, ..Stroke::default() }`),
    // and its `with_*` builders.
    for name in ["width", "style", "line_cap", "line_join", "line_dash"] {
        let path = format!("iced::widget::canvas::Stroke::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let mut stroke = read::<marker::Stroke>(receiver, &path)?;
                let [operand] = operands else {
                    return Ok(match name {
                        "width" => Dynamic::from(f64::from(stroke.width)),
                        "style" => Dynamic::from(stroke.style),
                        "line_cap" => Dynamic::from(stroke.line_cap),
                        "line_dash" => Dynamic::from(stroke.line_dash.clone()),
                        _ => Dynamic::from(stroke.line_join),
                    });
                };
                match name {
                    "width" => stroke.width = pixels(operand, &path)?,
                    "style" => stroke.style = read(operand, &path)?,
                    "line_cap" => stroke.line_cap = read(operand, &path)?,
                    "line_dash" => stroke.line_dash = read(operand, &path)?,
                    _ => stroke.line_join = read(operand, &path)?,
                }
                Ok(Dynamic::from(stroke))
            })
            .expect("the iced facade must retain every Stroke field");
    }
    for name in [
        "with_width",
        "with_color",
        "with_line_cap",
        "with_line_join",
    ] {
        let path = format!("iced::widget::canvas::Stroke::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [receiver, operand] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let mut stroke = read::<marker::Stroke>(receiver, &path)?;
                // iced's `with_*` builders, on the owned stroke.
                match name {
                    "with_width" => stroke.width = pixels(operand, &path)?,
                    "with_color" => {
                        stroke.style = canvas::Style::Solid(read(operand, &path)?);
                    }
                    "with_line_cap" => stroke.line_cap = read(operand, &path)?,
                    _ => stroke.line_join = read(operand, &path)?,
                }
                Ok(Dynamic::from(stroke))
            })
            .expect("the iced facade must retain every Stroke builder");
    }
    for name in ["style", "rule"] {
        let path = format!("iced::widget::canvas::Fill::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let mut fill = read::<canvas::Fill>(receiver, &path)?;
                let [operand] = operands else {
                    return Ok(match name {
                        "style" => Dynamic::from(fill.style),
                        _ => Dynamic::from(fill.rule),
                    });
                };
                match name {
                    "style" => fill.style = read(operand, &path)?,
                    _ => fill.rule = read(operand, &path)?,
                }
                Ok(Dynamic::from(fill))
            })
            .expect("the iced facade must retain every Fill field");
    }
    for name in [
        "content",
        "position",
        "max_width",
        "color",
        "size",
        "font",
        "align_x",
        "align_y",
        "shaping",
    ] {
        let path = format!("iced::widget::canvas::Text::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let mut text = read::<canvas::Text>(receiver, &path)?;
                let [operand] = operands else {
                    return Ok(match name {
                        "content" => {
                            Dynamic::from(rawr::ImmutableString::from(text.content.as_str()))
                        }
                        "position" => Dynamic::from(text.position),
                        "max_width" => Dynamic::from(f64::from(text.max_width)),
                        "color" => Dynamic::from(text.color),
                        "size" => Dynamic::from(f64::from(text.size.0)),
                        "font" => Dynamic::from(text.font),
                        "align_x" => Dynamic::from(text.align_x),
                        "shaping" => Dynamic::from(text.shaping),
                        _ => Dynamic::from(marker::Vertical(match text.align_y {
                            iced::alignment::Vertical::Top => iced::Alignment::Start,
                            iced::alignment::Vertical::Center => iced::Alignment::Center,
                            iced::alignment::Vertical::Bottom => iced::Alignment::End,
                        })),
                    });
                };
                match name {
                    "content" => {
                        text.content = read::<rawr::ImmutableString>(operand, &path)?.to_string()
                    }
                    "position" => text.position = point(operand, &path)?,
                    "max_width" => text.max_width = pixels(operand, &path)?,
                    "color" => text.color = read(operand, &path)?,
                    "size" => text.size = iced::Pixels(pixels(operand, &path)?),
                    "font" => text.font = read(operand, &path)?,
                    "align_x" => text.align_x = read(operand, &path)?,
                    "shaping" => text.shaping = read(operand, &path)?,
                    _ => text.align_y = read::<marker::Vertical>(operand, &path)?.0.into(),
                }
                Ok(Dynamic::from(text))
            })
            .expect("the iced facade must retain every canvas::Text field");
    }
}

fn bind_bounds(builder: &mut host::runtime::Builder) {
    builder
        .bind("iced::mouse::Cursor::Unavailable", |_| {
            Ok(Dynamic::from(iced::mouse::Cursor::Unavailable))
        })
        .expect("the iced facade must retain Cursor::Unavailable");
    builder
        .bind("iced::mouse::Cursor::Available", |args| {
            let [position] = args else {
                return Err(arity("Cursor::Available", 1, args.len()));
            };
            Ok(Dynamic::from(iced::mouse::Cursor::Available(point(
                position,
                "Cursor::Available",
            )?)))
        })
        .expect("the iced facade must retain Cursor::Available");
    for name in [
        "position",
        "position_in",
        "position_over",
        "position_from",
        "is_over",
    ] {
        let path = format!("iced::mouse::Cursor::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let cursor = read::<iced::mouse::Cursor>(receiver, &path)?;
                let optional = |point: Option<iced::Point>| match point {
                    Some(point) => rawr::option::some(Dynamic::from(point)),
                    None => rawr::option::none(),
                };
                Ok(match (name, &*operands) {
                    ("position", []) => optional(cursor.position()),
                    ("position_in", [bounds]) => optional(cursor.position_in(read(bounds, &path)?)),
                    ("position_over", [bounds]) => {
                        optional(cursor.position_over(read(bounds, &path)?))
                    }
                    ("position_from", [origin]) => {
                        optional(cursor.position_from(point(origin, &path)?))
                    }
                    ("is_over", [bounds]) => Dynamic::from(cursor.is_over(read(bounds, &path)?)),
                    _ => return Err(arity(&path, 2, operands.len() + 1)),
                })
            })
            .expect("the iced facade must retain every Cursor method");
    }
    for name in ["size", "center", "position", "contains"] {
        let path = format!("iced::Rectangle::{name}");
        builder
            .bind(path.clone(), move |args| {
                let Some((receiver, operands)) = args.split_first_mut() else {
                    return Err(arity(&path, 1, 0));
                };
                let bounds = read::<iced::Rectangle>(receiver, &path)?;
                Ok(match (name, &*operands) {
                    ("size", []) => Dynamic::from(bounds.size()),
                    ("center", []) => Dynamic::from(bounds.center()),
                    ("position", []) => Dynamic::from(bounds.position()),
                    ("contains", [inner]) => Dynamic::from(bounds.contains(point(inner, &path)?)),
                    _ => return Err(arity(&path, 1, operands.len() + 1)),
                })
            })
            .expect("the iced facade must retain every Rectangle method");
    }
    builder
        .bind("iced::Point::distance", |args| {
            let [from, to] = args else {
                return Err(arity("Point::distance", 2, args.len()));
            };
            let path = "Point::distance";
            Ok(Dynamic::from(f64::from(
                point(from, path)?.distance(point(to, path)?),
            )))
        })
        .expect("the iced facade must retain Point::distance");
    builder
        .bind("iced::Point::ORIGIN", |_| {
            Ok(Dynamic::from(iced::Point::ORIGIN))
        })
        .expect("the iced facade must retain Point::ORIGIN");
    builder
        .bind("iced::mouse::Interaction::default", |_| {
            Ok(Dynamic::from(iced::mouse::Interaction::default()))
        })
        .expect("the iced facade must retain Interaction::default");
}

impl Witnessed for marker::Canvas {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

impl super::Descriptor for marker::Canvas {
    fn lower<'a>(&self, context: &super::Context<'a>) -> iced::Element<'a, runtime::Message> {
        iced::widget::canvas(Program {
            program: self.program.clone(),
            context: context.clone(),
        })
        .width(self.width)
        .height(self.height)
        .boxed()
    }

    fn identity(&self) -> super::Identity {
        self.identity.clone()
    }
}

/// A script program as iced's `canvas::Program`.
struct Program<'a> {
    program: marker::CanvasProgram,
    context: super::Context<'a>,
}

/// The tree-held State: the script State once `update` has changed it,
/// the failure of the last `update`, which `draw` shows, and iced's own
/// geometry for each recording the last `draw` returned.
#[derive(Default)]
pub(crate) struct State {
    value: Option<Dynamic>,
    failure: Option<String>,
    /// A script `Cache` returns the same recording until it is cleared, so
    /// iced tessellates that recording once, not on every draw.
    geometries: std::cell::RefCell<Vec<(Arc<[marker::Draw]>, canvas::Cache)>>,
}

impl Program<'_> {
    fn state(&self, state: &State) -> Dynamic {
        state
            .value
            .clone()
            .unwrap_or_else(|| self.program.state.clone())
    }

    fn theme(&self) -> theme::Value {
        self.context.theme().clone()
    }
}

impl canvas::Program<runtime::Message> for Program<'_> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<runtime::Message>> {
        let context = &self.context;
        let mut arguments = [
            self.program.value.clone(),
            self.state(state),
            Dynamic::from(event.clone()),
            Dynamic::from(bounds),
            Dynamic::from(cursor),
        ];
        // `update` copies its `&mut State` back out: `[action, state]`.
        let changed = context
            .invoke(&self.program.update, &mut arguments)?
            .and_then(|changed| {
                let changed = changed
                    .try_cast_result::<rawr::Array>()
                    .map_err(|value| format!("returned `{}`", value.type_name()))?;
                let [mut action, value] = <[Dynamic; 2]>::try_from(changed)
                    .map_err(|changed| format!("returned {} values", changed.len()))?;
                let action = rawr::option::take(&mut action)
                    .ok_or_else(|| format!("returned `{}`", action.type_name()))?
                    .map(|mut action| {
                        take::<marker::CanvasAction>(&mut action, "iced::widget::canvas::Action")
                            .map_err(|error| error.to_string())
                    })
                    .transpose()?;
                Ok((action, value))
            });
        match changed {
            Ok((action, value)) => {
                state.value = Some(value);
                state.failure = None;
                action.map(|action| {
                    let native = match action.message {
                        Some(message) => canvas::Action::publish(context.message(message)),
                        None if action.redraw => canvas::Action::request_redraw(),
                        None => canvas::Action::capture(),
                    };
                    if action.capture {
                        native.and_capture()
                    } else {
                        native
                    }
                })
            }
            Err(error) => {
                state.failure = Some(format!("canvas update failed: {error}"));
                Some(canvas::Action::request_redraw())
            }
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let failure = |message: String| {
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            frame.fill_text(canvas::Text {
                content: message,
                color: iced::Color::from_rgb(0.9, 0.2, 0.2),
                max_width: bounds.width,
                ..canvas::Text::default()
            });
            vec![frame.into_geometry()]
        };
        if let Some(message) = &state.failure {
            return failure(message.clone());
        }
        let context = &self.context;
        let mut arguments = [
            self.program.value.clone(),
            self.state(state),
            Dynamic::from(marker::Renderer),
            Dynamic::from(self.theme()),
            Dynamic::from(bounds),
            Dynamic::from(cursor),
        ];
        let drawn = match context.invoke(&self.program.draw, &mut arguments) {
            None => return Vec::new(),
            Some(Ok(drawn)) => drawn,
            Some(Err(error)) => return failure(format!("canvas draw failed: {error}")),
        };
        let Ok(geometries) = drawn.into_typed_array::<Dynamic>() else {
            return failure("canvas draw returned something other than a `Vec<Geometry>`".into());
        };
        let mut previous = std::mem::take(&mut *state.geometries.borrow_mut());
        let mut kept = Vec::with_capacity(geometries.len());
        let drawn = geometries
            .into_iter()
            .filter_map(|geometry| geometry.try_cast::<marker::Geometry>())
            .map(|geometry| {
                let cache = previous
                    .iter()
                    .position(|(draws, _)| Arc::ptr_eq(draws, &geometry.draws))
                    .map(|index| previous.swap_remove(index).1)
                    .unwrap_or_default();
                let drawn = cache.draw(renderer, geometry.size, |frame| {
                    for draw in geometry.draws.iter() {
                        replay(frame, draw);
                    }
                });
                kept.push((geometry.draws, cache));
                drawn
            })
            .collect();
        *state.geometries.borrow_mut() = kept;
        drawn
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: iced::Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        let context = &self.context;
        let mut arguments = [
            self.program.value.clone(),
            self.state(state),
            Dynamic::from(bounds),
            Dynamic::from(cursor),
        ];
        match context.invoke(&self.program.interaction, &mut arguments) {
            Some(Ok(interaction)) => interaction
                .try_cast::<iced::mouse::Interaction>()
                .unwrap_or_default(),
            _ => iced::mouse::Interaction::default(),
        }
    }
}

fn replay(frame: &mut canvas::Frame, draw: &marker::Draw) {
    match draw {
        marker::Draw::Fill(path, fill) => frame.fill(path, *fill),
        marker::Draw::FillRectangle(top_left, size, fill) => {
            frame.fill_rectangle(*top_left, *size, *fill);
        }
        marker::Draw::Stroke(path, stroke) => frame.stroke(path, stroke.native()),
        marker::Draw::StrokeRectangle(top_left, size, stroke) => {
            frame.stroke_rectangle(*top_left, *size, stroke.native());
        }
        marker::Draw::FillText(text) => frame.fill_text(text.clone()),
        marker::Draw::PushTransform => frame.push_transform(),
        marker::Draw::PopTransform => frame.pop_transform(),
        marker::Draw::Translate(translation) => frame.translate(*translation),
        marker::Draw::Rotate(angle) => frame.rotate(*angle),
        marker::Draw::Scale(scale) => frame.scale(*scale),
        marker::Draw::ScaleNonuniform(scale) => frame.scale_nonuniform(*scale),
        #[cfg(feature = "image")]
        marker::Draw::Image(bounds, image) => frame.draw_image(*bounds, image.clone()),
    }
}
