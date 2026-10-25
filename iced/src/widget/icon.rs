//! iced's `text::Shaping`, `text::LineHeight` and `checkbox::Icon`.

use iced::widget::checkbox::Icon;
use iced::widget::text::{LineHeight, Shaping};
use rawr::host;
use rawr::{Dynamic, EvalAltResult};

use super::builtin::{arity, pixels, runtime_error, take};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    for (name, shaping) in
        crate::TEXT_SHAPINGS
            .into_iter()
            .zip([Shaping::Auto, Shaping::Basic, Shaping::Advanced])
    {
        builder
            .bind(format!("iced::widget::text::Shaping::{name}"), move |_| {
                Ok(Dynamic::from(shaping))
            })
            .expect("the iced facade must retain every text::Shaping");
    }
    for (name, make) in [
        (
            "Relative",
            (|value: f32| LineHeight::Relative(value)) as fn(f32) -> LineHeight,
        ),
        ("Absolute", |value| LineHeight::Absolute(value.into())),
    ] {
        let path = format!("iced::widget::text::LineHeight::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [value] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                Ok(Dynamic::from(make(pixels(value, &path)?)))
            })
            .expect("the iced facade must retain every text::LineHeight variant");
    }
    builder
        .bind("iced::widget::checkbox::Icon::default", |args| {
            if !args.is_empty() {
                return Err(arity("checkbox::Icon::default", 0, args.len()));
            }
            Ok(Dynamic::from(Icon {
                font: iced::Font::DEFAULT,
                code_point: ' ',
                size: None,
                line_height: None,
                shaping: Shaping::Basic,
            }))
        })
        .expect("the iced facade must retain checkbox::Icon::default");
    for field in crate::CHECKBOX_ICON_FIELDS.map(|(field, _)| field) {
        let path = format!("iced::widget::checkbox::Icon::{field}");
        builder
            .bind(path.clone(), move |args| match args {
                [icon] => read(
                    &*icon.read_lock::<Icon>().ok_or_else(|| expected(&path))?,
                    field,
                ),
                [icon, value] => {
                    let mut icon = take::<Icon>(icon, "iced::widget::checkbox::Icon")?;
                    write(&mut icon, field, value, &path)?;
                    Ok(Dynamic::from(icon))
                }
                _ => Err(arity(&path, 2, args.len())),
            })
            .expect("the iced facade must retain every checkbox::Icon field");
    }
}

fn read(icon: &Icon, field: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    Ok(match field {
        "font" => Dynamic::from(icon.font),
        "code_point" => Dynamic::from(icon.code_point),
        "size" => rawr::option::from(icon.size.map(|size| f64::from(size.0))),
        "line_height" => rawr::option::from(icon.line_height),
        _ => Dynamic::from(icon.shaping),
    })
}

fn write(
    icon: &mut Icon,
    field: &str,
    value: &mut Dynamic,
    path: &str,
) -> Result<(), Box<EvalAltResult>> {
    match field {
        "font" => icon.font = take(value, "iced::Font")?,
        "code_point" => {
            icon.code_point = value
                .as_char()
                .map_err(|_| runtime_error(format!("{path} expects a `char`")))?;
        }
        "size" => {
            icon.size = match rawr::option::take(value) {
                Some(Some(size)) => Some(pixels(&size, path)?.into()),
                Some(None) => None,
                None => Some(pixels(value, path)?.into()),
            };
        }
        "line_height" => {
            icon.line_height = super::rich::maybe(value, "iced::widget::text::LineHeight")?;
        }
        _ => icon.shaping = take(value, "iced::widget::text::Shaping")?,
    }
    Ok(())
}

fn expected(path: &str) -> Box<EvalAltResult> {
    runtime_error(format!("{path} expects a `checkbox::Icon`"))
}
