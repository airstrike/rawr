//! iced's `scrollable::Anchor`, `Scrollbar`, `Direction`, `Scroll`,
//! `Viewport` and `Source`, and the `Scrollable` builders that take them.

use iced::widget::scrollable::{Anchor, Direction, Scroll, Scrollbar, Source, Viewport};
use rawr::host;
use rawr::Dynamic;

use super::builtin::{arity, exact_bool, pixels, runtime_error, take};
use crate::marker;

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    for (name, anchor) in crate::SCROLLABLE_ANCHORS
        .into_iter()
        .zip([Anchor::Start, Anchor::End])
    {
        builder
            .bind(format!("iced::widget::scrollable::Anchor::{name}"), move |_| {
                Ok(Dynamic::from(anchor))
            })
            .expect("the iced facade must retain every scrollable::Anchor");
    }
    for (name, source) in crate::SCROLLABLE_SOURCES.into_iter().zip([
        Source::Wheel,
        Source::Touch,
        Source::Scrollbar,
        Source::AutoScroll,
        Source::Operation,
        Source::Action,
        Source::Content,
        Source::Resize,
    ]) {
        builder
            .bind(format!("iced::widget::scrollable::Source::{name}"), move |_| {
                Ok(Dynamic::from(source))
            })
            .expect("the iced facade must retain every scrollable::Source");
    }
    for (path, value) in [
        (
            "iced::widget::scrollable::Anchor::default",
            (|| Dynamic::from(Anchor::default())) as fn() -> Dynamic,
        ),
        ("iced::widget::scrollable::Scrollbar::default", || {
            Dynamic::from(Scrollbar::default())
        }),
        ("iced::widget::scrollable::Scrollbar::new", || {
            Dynamic::from(Scrollbar::new())
        }),
        ("iced::widget::scrollable::Scrollbar::hidden", || {
            Dynamic::from(Scrollbar::hidden())
        }),
        ("iced::widget::scrollable::Direction::default", || {
            Dynamic::from(Direction::default())
        }),
        ("iced::widget::scrollable::Direction::Both::default", || {
            Dynamic::from(marker::ScrollBoth::default())
        }),
    ] {
        builder
            .bind(path, move |args| {
                if !args.is_empty() {
                    return Err(arity(path, 0, args.len()));
                }
                Ok(value())
            })
            .expect("the iced facade must retain every scrollable constructor");
    }
    for name in ["anchor", "width", "margin", "scroller_width", "spacing", "padding"] {
        let path = format!("iced::widget::scrollable::Scrollbar::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [scrollbar, value] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let scrollbar = take::<Scrollbar>(scrollbar, "iced::widget::scrollable::Scrollbar")?;
                Ok(Dynamic::from(match name {
                    "anchor" => scrollbar.anchor(take(value, "iced::widget::scrollable::Anchor")?),
                    "width" => scrollbar.width(pixels(value, &path)?),
                    "margin" => scrollbar.margin(pixels(value, &path)?),
                    "scroller_width" => scrollbar.scroller_width(pixels(value, &path)?),
                    "spacing" => scrollbar.spacing(pixels(value, &path)?),
                    _ => scrollbar.padding(pixels(value, &path)?),
                }))
            })
            .expect("the iced facade must retain every Scrollbar builder");
    }
    for (name, variant) in [
        ("Vertical", Direction::Vertical as fn(Scrollbar) -> Direction),
        ("Horizontal", Direction::Horizontal),
    ] {
        let path = format!("iced::widget::scrollable::Direction::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [scrollbar] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                Ok(Dynamic::from(variant(take(
                    scrollbar,
                    "iced::widget::scrollable::Scrollbar",
                )?)))
            })
            .expect("the iced facade must retain every scrollable::Direction variant");
    }
    for field in ["horizontal", "vertical"] {
        let path = format!("iced::widget::scrollable::Direction::Both::{field}");
        builder
            .bind(path.clone(), move |args| match args {
                [both] => {
                    let both = both
                        .read_lock::<marker::ScrollBoth>()
                        .map(|both| *both)
                        .ok_or_else(|| runtime_error(format!("{path} expects a `Direction::Both`")))?;
                    Ok(Dynamic::from(match field {
                        "horizontal" => both.horizontal,
                        _ => both.vertical,
                    }))
                }
                [both, scrollbar] => {
                    let mut both = take::<marker::ScrollBoth>(
                        both,
                        "iced::widget::scrollable::Direction::Both",
                    )?;
                    let scrollbar = take(scrollbar, "iced::widget::scrollable::Scrollbar")?;
                    match field {
                        "horizontal" => both.horizontal = scrollbar,
                        _ => both.vertical = scrollbar,
                    }
                    Ok(Dynamic::from(both))
                }
                _ => Err(arity(&path, 2, args.len())),
            })
            .expect("the iced facade must retain every Direction::Both field");
    }
    builder
        .bind("iced::widget::scrollable::Direction::Both::into", |args| {
            let [both] = args else {
                return Err(arity("Direction::Both::into", 1, args.len()));
            };
            let both = take::<marker::ScrollBoth>(both, "iced::widget::scrollable::Direction::Both")?;
            Ok(Dynamic::from(Direction::from(both)))
        })
        .expect("the iced facade must retain Direction::Both::into");
    for name in ["direction", "spacing", "auto_scroll", "on_scroll"] {
        let path = format!("iced::widget::Scrollable::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [scrollable, value] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let mut scrollable =
                    take::<marker::Scrollable>(scrollable, "iced::widget::Scrollable")?;
                match name {
                    "direction" => {
                        scrollable.direction =
                            Some(take(value, "iced::widget::scrollable::Direction")?);
                    }
                    "spacing" => scrollable.spacing = Some(pixels(value, &path)?),
                    "auto_scroll" => scrollable.auto_scroll = Some(exact_bool(value, &path)?),
                    _ => {
                        scrollable.on_scroll =
                            Some(take::<rawr::item::Value>(value, "symbolic message mapper")?);
                    }
                }
                Ok(Dynamic::from(scrollable))
            })
            .expect("the iced facade must retain every Scrollable builder");
    }
    for (path, read) in [
        (
            "iced::widget::scrollable::Scroll::viewport",
            (|scroll: &Scroll| Dynamic::from(scroll.viewport)) as fn(&Scroll) -> Dynamic,
        ),
        ("iced::widget::scrollable::Scroll::source", |scroll| {
            Dynamic::from(scroll.source)
        }),
    ] {
        builder
            .bind(path, move |args| {
                let [scroll] = args else {
                    return Err(arity(path, 1, args.len()));
                };
                let scroll = scroll
                    .read_lock::<Scroll>()
                    .ok_or_else(|| runtime_error(format!("{path} expects a `Scroll`")))?;
                Ok(read(&scroll))
            })
            .expect("the iced facade must retain every Scroll field");
    }
    for (path, read) in [
        (
            "iced::widget::scrollable::Viewport::relative_offset",
            (|viewport: &Viewport| Dynamic::from(viewport.relative_offset()))
                as fn(&Viewport) -> Dynamic,
        ),
        ("iced::widget::scrollable::Viewport::absolute_offset", |viewport| {
            Dynamic::from(viewport.absolute_offset())
        }),
    ] {
        builder
            .bind(path, move |args| {
                let [viewport] = args else {
                    return Err(arity(path, 1, args.len()));
                };
                let viewport = viewport
                    .read_lock::<Viewport>()
                    .ok_or_else(|| runtime_error(format!("{path} expects a `Viewport`")))?;
                Ok(read(&viewport))
            })
            .expect("the iced facade must retain every Viewport reader");
    }
}
