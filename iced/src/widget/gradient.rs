//! iced's `gradient::Linear`, which a container style or a background is
//! made from.

use iced::gradient::Linear;
use iced::{Color, Radians};
use rawr::host;
use rawr::Dynamic;

use super::builtin::{arity, number, take};

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    // `Linear::new(angle)`: iced's `impl Into<Radians>`, radians as a
    // number or an angle.
    builder
        .bind("iced::gradient::Linear::new", |args| {
            let [angle] = args else {
                return Err(arity("gradient::Linear::new", 1, args.len()));
            };
            let angle = match angle.as_float() {
                Ok(radians) => Radians(radians as f32),
                Err(_) => super::angle::radians(angle)?,
            };
            Ok(Dynamic::from(Linear::new(angle)))
        })
        .expect("the iced facade must retain gradient::Linear::new");
    builder
        .bind("iced::gradient::Linear::add_stop", |args| {
            let [linear, offset, color] = args else {
                return Err(arity("gradient::Linear::add_stop", 3, args.len()));
            };
            let linear = take::<Linear>(linear, "iced::gradient::Linear")?;
            let offset = number(offset, "gradient::Linear::add_stop")? as f32;
            let color = take::<Color>(color, "iced::Color")?;
            Ok(Dynamic::from(linear.add_stop(offset, color)))
        })
        .expect("the iced facade must retain gradient::Linear::add_stop");
    // `linear.into()`: the container style whose background it is.
    builder
        .bind("iced::gradient::Linear::into", |args| {
            let [linear] = args else {
                return Err(arity("gradient::Linear::into", 1, args.len()));
            };
            let linear = take::<Linear>(linear, "iced::gradient::Linear")?;
            Ok(Dynamic::from(iced::widget::container::Style::from(linear)))
        })
        .expect("the iced facade must retain gradient::Linear::into");
    // `Background::from(linear)`, and `linear.into()` where a `Background`
    // is expected.
    builder
        .bind("iced::Background::from", |args| {
            let [linear] = args else {
                return Err(arity("Background::from", 1, args.len()));
            };
            let linear = take::<Linear>(linear, "iced::gradient::Linear")?;
            Ok(Dynamic::from(iced::Background::from(linear)))
        })
        .expect("the iced facade must retain Background::from");
}
