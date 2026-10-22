//! iced master's small helpers: alignment shorthands, `left`/`right`/`top`/
//! `bottom`, `rule`, `stack`, `color!`, `time::hours`/`minutes` and `font`.

use iced::widget::Widget as _;
use rawr::host;
use rawr::{Dynamic, EvalAltResult};

use super::builtin::{
    arity, length, positional_row, project, runtime_error, take, take_children,
    take_container, witness_error, Witnessed,
};
use crate::{marker, runtime, widget};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    bind_alignment(builder);
    bind_containers(builder);
    bind_rule(builder);
    bind_stack(builder);
    bind_values(builder);
    bind_font(builder);
}

fn bind_alignment(builder: &mut host::runtime::Builder) {
    for (name, alignment) in [
        ("Top", iced::Alignment::Start),
        ("Center", iced::Alignment::Center),
        ("Bottom", iced::Alignment::End),
    ] {
        builder
            .bind(format!("iced::alignment::Vertical::{name}"), move |_| {
                Ok(Dynamic::from(marker::Vertical(alignment)))
            })
            .expect("the iced facade must retain every alignment::Vertical value");
    }
    for (name, alignment) in [("Left", iced::Alignment::Start), ("Right", iced::Alignment::End)] {
        builder
            .bind(format!("iced::{name}"), move |_| {
                Ok(Dynamic::from(marker::Horizontal(alignment)))
            })
            .expect("the iced facade must retain every horizontal shorthand");
    }
    for (name, alignment) in [("Top", iced::Alignment::Start), ("Bottom", iced::Alignment::End)] {
        builder
            .bind(format!("iced::{name}"), move |_| {
                Ok(Dynamic::from(marker::Vertical(alignment)))
            })
            .expect("the iced facade must retain every vertical shorthand");
    }
}

/// `left(content)` and kin: a container filling one axis, its content
/// aligned to that side, as iced's `container(content).align_right(Fill)`.
fn bind_containers(builder: &mut host::runtime::Builder) {
    for (name, x, y) in [
        ("left", Some(iced::Alignment::Start), None),
        ("right", Some(iced::Alignment::End), None),
        ("top", None, Some(iced::Alignment::Start)),
        ("bottom", None, Some(iced::Alignment::End)),
    ] {
        builder
            .bind_with_context(format!("iced::widget::{name}"), move |call, args| {
                if args.len() != 1 {
                    return Err(arity(name, 1, args.len()));
                }
                let mut container = marker::Container {
                    witness: call.output_witness().map_err(witness_error)?,
                    identity: widget::Identity::unique(),
                    child: take::<widget::Value>(args[0], "iced::Element")?,
                    padding: iced::Padding::ZERO,
                    width: iced::Length::Fit,
                    height: iced::Length::Fit,
                    align_x: iced::Alignment::Start,
                    align_y: iced::Alignment::Start,
                    style: None,
                };
                if let Some(x) = x {
                    container.width = iced::Length::Fill;
                    container.align_x = x;
                }
                if let Some(y) = y {
                    container.height = iced::Length::Fill;
                    container.align_y = y;
                }
                Ok(Dynamic::from(container))
            })
            .expect("the iced facade must retain every side container");
    }
    type Set = fn(&mut marker::Container, iced::Length);
    for (name, set) in [
        (
            "center",
            (|container, length| {
                container.width = length;
                container.height = length;
                container.align_x = iced::Alignment::Center;
                container.align_y = iced::Alignment::Center;
            }) as Set,
        ),
        ("align_left", |container, length| {
            container.width = length;
            container.align_x = iced::Alignment::Start;
        }),
        ("align_right", |container, length| {
            container.width = length;
            container.align_x = iced::Alignment::End;
        }),
        ("align_top", |container, length| {
            container.height = length;
            container.align_y = iced::Alignment::Start;
        }),
        ("align_bottom", |container, length| {
            container.height = length;
            container.align_y = iced::Alignment::End;
        }),
    ] {
        let path = format!("iced::widget::Container::{name}");
        builder
            .bind(path.clone(), move |args| {
                let mut container = take_container(args, &path)?;
                set(&mut container, length(args[1])?);
                Ok(Dynamic::from(container))
            })
            .expect("the iced facade must retain every Container side builder");
    }
}

impl widget::Descriptor for marker::Native {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        (self.lower)(context)
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

/// An Element lowered by `lower`, as a facade function's output.
pub(super) fn native(
    call: &host::runtime::Call<'_>,
    lower: impl Fn(&widget::Context<'_>) -> iced::Element<'static, runtime::Message>
        + Send
        + Sync
        + 'static,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let native = marker::Native {
        identity: widget::Identity::unique(),
        lower: std::sync::Arc::new(lower),
    };
    widget::Value::from_factory(native, call)
        .map(Dynamic::from)
        .map_err(witness_error)
}

/// `rule::horizontal(height)` and `rule::vertical(width)`.
fn bind_rule(builder: &mut host::runtime::Builder) {
    for (name, horizontal) in [("horizontal", true), ("vertical", false)] {
        builder
            .bind_with_context(format!("iced::widget::rule::{name}"), move |call, args| {
                if args.len() != 1 {
                    return Err(arity(name, 1, args.len()));
                }
                let thickness = super::builtin::pixels(args[0], name)?;
                native(call, move |_| {
                    if horizontal {
                        iced::widget::rule::horizontal(thickness).boxed()
                    } else {
                        iced::widget::rule::vertical(thickness).boxed()
                    }
                })
            })
            .expect("the iced facade must retain every rule");
    }
}

impl widget::Descriptor for marker::Stack {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        iced::widget::Stack::with_children(self.children.iter().map(|child| child.lower(context)))
            .width(self.width)
            .height(self.height)
            .boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

impl Witnessed for marker::Stack {
    fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }
}

fn new_stack(
    call: &host::runtime::Call<'_>,
    children: Vec<widget::Value>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    if children.iter().any(|child| child.key().is_some()) {
        return Err(positional_row());
    }
    Ok(Dynamic::from(marker::Stack {
        witness: call.output_witness().map_err(witness_error)?,
        identity: widget::Identity::unique(),
        children,
        width: iced::Length::Shrink,
        height: iced::Length::Shrink,
    }))
}

fn bind_stack(builder: &mut host::runtime::Builder) {
    builder
        .bind_type_attestation("iced::widget::Stack", |value, expected| {
            let value = value.read_lock::<marker::Stack>()?;
            (expected.argument_count() == 1)
                .then(|| expected.attest_witness(Some(&value.witness)))
                .flatten()
        })
        .expect("the iced facade must declare generic widget::Stack exactly once");
    builder
        .bind_projection_with_context("iced::widget::Stack", "iced::Element", |call, args| {
            project::<marker::Stack>(call, args, "iced::widget::Stack")
        })
        .expect("the iced facade must retain the exact Stack to Element projection");
    builder
        .bind_macro_with_context("iced::widget::stack", |call, args| {
            let children = args
                .iter_mut()
                .map(|value| take::<widget::Value>(value, "iced::Element"))
                .collect::<Result<Vec<_>, _>>()?;
            new_stack(call, children)
        })
        .expect("the iced facade must retain the exact stack macro");
    for path in [
        "iced::widget::stack",
        "iced::widget::Stack::with_children",
        "iced::widget::Stack::from_vec",
    ] {
        builder
            .bind_with_context(path, |call, args| {
                if args.len() != 1 {
                    return Err(arity("stack", 1, args.len()));
                }
                new_stack(call, take_children(args[0])?)
            })
            .expect("the iced facade must retain the exact stack functions");
    }
    builder
        .bind_with_context("iced::widget::Stack::new", |call, _| new_stack(call, Vec::new()))
        .expect("the iced facade must retain the exact Stack::new item");
    for (name, height) in [("width", false), ("height", true)] {
        let path = format!("iced::widget::Stack::{name}");
        builder
            .bind(path.clone(), move |args| {
                if args.len() != 2 {
                    return Err(arity(&path, 2, args.len()));
                }
                let mut stack = take::<marker::Stack>(args[0], "iced::widget::Stack")?;
                let value = length(args[1])?;
                if height {
                    stack.height = value;
                } else {
                    stack.width = value;
                }
                Ok(Dynamic::from(stack))
            })
            .expect("the iced facade must retain every Stack sizing item");
    }
    builder
        .bind("iced::widget::Stack::push", |args| {
            if args.len() != 2 {
                return Err(arity("Stack::push", 2, args.len()));
            }
            let mut stack = take::<marker::Stack>(args[0], "iced::widget::Stack")?;
            let child = take::<widget::Value>(args[1], "iced::Element")?;
            if child.key().is_some() {
                return Err(positional_row());
            }
            stack.children.push(child);
            Ok(Dynamic::from(stack))
        })
        .expect("the iced facade must retain the exact Stack::push item");
    builder
        .bind("iced::widget::Stack::extend", |args| {
            if args.len() != 2 {
                return Err(arity("Stack::extend", 2, args.len()));
            }
            let mut stack = take::<marker::Stack>(args[0], "iced::widget::Stack")?;
            let children = take_children(args[1])?;
            if children.iter().any(|child| child.key().is_some()) {
                return Err(positional_row());
            }
            stack.children.extend(children);
            Ok(Dynamic::from(stack))
        })
        .expect("the iced facade must retain the exact Stack::extend item");
}

/// `time::hours`/`minutes` and `color!(0xrrggbb)` / `color!(r, g, b)`.
fn bind_values(builder: &mut host::runtime::Builder) {
    for (name, seconds) in [("hours", 3600_u64), ("minutes", 60)] {
        builder
            .bind(format!("iced::time::{name}"), move |args| {
                let amount = args
                    .first()
                    .and_then(|value| value.as_int().ok())
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or_else(|| runtime_error(format!("time::{name} expects a `u64`")))?;
                Ok(Dynamic::from(std::time::Duration::from_secs(
                    amount.saturating_mul(seconds),
                )))
            })
            .expect("the iced facade must retain every time unit");
    }
    builder
        .bind_macro("iced::color", |args| {
            let channels = args
                .iter()
                .map(|value| value.as_int())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|actual| {
                    runtime_error(format!("color! expects integers, found `{actual}`"))
                })?;
            let channel = |value: i64| {
                u8::try_from(value)
                    .map_err(|_| runtime_error(format!("color! channel {value} is not a `u8`")))
            };
            match channels.as_slice() {
                [hex] => {
                    let hex = u32::try_from(*hex).ok().filter(|hex| *hex <= 0x00ff_ffff).ok_or_else(
                        || runtime_error(format!("color! expects `0xrrggbb`, found {hex:#x}")),
                    )?;
                    Ok(Dynamic::from(iced::Color::from_rgb8(
                        (hex >> 16) as u8,
                        (hex >> 8) as u8,
                        hex as u8,
                    )))
                }
                [r, g, b] => Ok(Dynamic::from(iced::Color::from_rgb8(
                    channel(*r)?,
                    channel(*g)?,
                    channel(*b)?,
                ))),
                _ => Err(runtime_error(format!(
                    "color! takes `0xrrggbb` or `r, g, b`, found {} values",
                    channels.len()
                ))),
            }
        })
        .expect("the iced facade must retain the exact color macro");
}

fn bind_font(builder: &mut host::runtime::Builder) {
    use iced::font::{Family, Stretch, Style, Weight};

    for (name, value) in crate::FONT_WEIGHTS.into_iter().zip([
        Weight::Thin,
        Weight::ExtraLight,
        Weight::Light,
        Weight::Normal,
        Weight::Medium,
        Weight::Semibold,
        Weight::Bold,
        Weight::ExtraBold,
        Weight::Black,
    ]) {
        builder
            .bind(format!("iced::font::Weight::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Weight");
    }
    for (name, value) in crate::FONT_STRETCHES.into_iter().zip([
        Stretch::UltraCondensed,
        Stretch::ExtraCondensed,
        Stretch::Condensed,
        Stretch::SemiCondensed,
        Stretch::Normal,
        Stretch::SemiExpanded,
        Stretch::Expanded,
        Stretch::ExtraExpanded,
        Stretch::UltraExpanded,
    ]) {
        builder
            .bind(format!("iced::font::Stretch::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Stretch");
    }
    for (name, value) in crate::FONT_STYLES.into_iter().zip([
        Style::Normal,
        Style::Italic,
        Style::Oblique,
    ]) {
        builder
            .bind(format!("iced::font::Style::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Style");
    }
    for (name, value) in crate::FONT_FAMILIES.into_iter().zip([
        Family::Serif,
        Family::SansSerif,
        Family::Cursive,
        Family::Fantasy,
        Family::Monospace,
    ]) {
        builder
            .bind(format!("iced::font::Family::{name}"), move |_| Ok(Dynamic::from(value)))
            .expect("the iced facade must retain every font::Family");
    }
    builder
        .bind("iced::font::Family::name", |args| {
            let name = super::builtin::exact_string(
                args.first().ok_or_else(|| arity("Family::name", 1, 0))?,
                "Family::name",
            )?;
            Ok(Dynamic::from(Family::name(&name)))
        })
        .expect("the iced facade must retain font::Family::name");
    builder
        .bind("iced::Font::new", |args| {
            let name = super::builtin::exact_string(
                args.first().ok_or_else(|| arity("Font::new", 1, 0))?,
                "Font::new",
            )?;
            Ok(Dynamic::from(iced::Font::with_family(Family::name(&name))))
        })
        .expect("the iced facade must retain Font::new");
    builder
        .bind("iced::Font::with_family", |args| {
            let family = take::<Family>(
                args.first_mut().ok_or_else(|| arity("Font::with_family", 1, 0))?,
                "iced::font::Family",
            )?;
            Ok(Dynamic::from(iced::Font::with_family(family)))
        })
        .expect("the iced facade must retain Font::with_family");
    builder
        .bind("iced::Font::weight", |args| {
            let (font, value) = font_and::<Weight>(args, "Font::weight", "iced::font::Weight")?;
            Ok(Dynamic::from(font.weight(value)))
        })
        .expect("the iced facade must retain Font::weight");
    builder
        .bind("iced::Font::stretch", |args| {
            let (font, value) = font_and::<Stretch>(args, "Font::stretch", "iced::font::Stretch")?;
            Ok(Dynamic::from(font.stretch(value)))
        })
        .expect("the iced facade must retain Font::stretch");
    builder
        .bind("iced::Font::style", |args| {
            let (font, value) = font_and::<Style>(args, "Font::style", "iced::font::Style")?;
            Ok(Dynamic::from(font.style(value)))
        })
        .expect("the iced facade must retain Font::style");
    builder
        .bind("iced::Font::family", |args| {
            let (mut font, value) = font_and::<Family>(args, "Font::family", "iced::font::Family")?;
            font.family = value;
            Ok(Dynamic::from(font))
        })
        .expect("the iced facade must retain Font::family");
}

fn font_and<T: Clone + 'static>(
    args: &mut [&mut Dynamic],
    operation: &str,
    expected: &str,
) -> Result<(iced::Font, T), Box<EvalAltResult>> {
    if args.len() != 2 {
        return Err(arity(operation, 2, args.len()));
    }
    let font = take::<iced::Font>(args[0], "iced::Font")?;
    let value = take::<T>(args[1], expected)?;
    Ok((font, value))
}
