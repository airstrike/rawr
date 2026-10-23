//! iced's `Degrees` and `Radians`.

use iced::{Degrees, Radians};
use rawr::host;
use rawr::{Dynamic, EvalAltResult};

use super::builtin::{arity, pixels, runtime_error, take};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    // `Degrees(x)` is `Degrees::from(x)`.
    builder
        .bind("iced::Degrees::from", |args| {
            let [value] = args else {
                return Err(arity("Degrees::from", 1, args.len()));
            };
            Ok(Dynamic::from(Degrees(pixels(value, "Degrees::from")?)))
        })
        .expect("the iced facade must retain Degrees::from");
    builder
        .bind("iced::Radians::from", |args| {
            let [value] = args else {
                return Err(arity("Radians::from", 1, args.len()));
            };
            if value.is::<Degrees>() {
                return Ok(Dynamic::from(radians(value)?));
            }
            Ok(Dynamic::from(Radians(pixels(value, "Radians::from")?)))
        })
        .expect("the iced facade must retain Radians::from");
    builder
        .bind("iced::Radians::PI", |_| Ok(Dynamic::from(Radians::PI)))
        .expect("the iced facade must retain Radians::PI");
    builder
        .bind("iced::Degrees::mul", |args| {
            let [degrees, factor] = args else {
                return Err(arity("Degrees::mul", 2, args.len()));
            };
            let degrees = take::<Degrees>(degrees, "iced::Degrees")?;
            Ok(Dynamic::from(degrees * pixels(factor, "Degrees::mul")?))
        })
        .expect("the iced facade must retain Degrees::mul");
    for name in ["add", "sub", "mul", "div", "rem"] {
        let path = format!("iced::Radians::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [left, right] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let left = take::<Radians>(left, "iced::Radians")?;
                let angle = right.is::<Radians>() || right.is::<Degrees>();
                let number = (!angle).then(|| pixels(right, &path)).transpose()?;
                let right = if angle { radians(right)? } else { Radians(0.0) };
                Ok(Dynamic::from(match (name, number) {
                    ("add", _) => left + right,
                    ("sub", _) => left - right,
                    ("mul", Some(number)) => left * number,
                    ("mul", None) => left * right,
                    ("div", Some(number)) => left / number,
                    ("div", None) => left / right,
                    _ => left % right,
                }))
            })
            .expect("the iced facade must retain every Radians operator");
    }
}

/// A `Radians`, or a `Degrees` in radians.
fn radians(value: &mut Dynamic) -> Result<Radians, Box<EvalAltResult>> {
    if let Some(degrees) = value.read_lock::<Degrees>().map(|degrees| *degrees) {
        return Ok(degrees.into());
    }
    if let Some(radians) = value.read_lock::<Radians>().map(|radians| *radians) {
        return Ok(radians);
    }
    Err(runtime_error("expected `Radians` or `Degrees`".into()))
}
