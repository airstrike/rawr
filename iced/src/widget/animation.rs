//! iced's `Animation<T>` of a `bool` or an `f32`, and its `Easing`.

use std::time::Duration;

use rawr::host;
use rawr::{Dynamic, EvalAltResult};

use super::builtin::{arity, number, runtime_error, take};
use crate::marker::{Animation, Easing, Instant};

/// The easings in `crate::EASINGS` order.
const EASINGS: [iced::animation::Easing; 31] = {
    use iced::animation::Easing::*;
    [
        Linear,
        EaseIn,
        EaseOut,
        EaseInOut,
        EaseInQuad,
        EaseOutQuad,
        EaseInOutQuad,
        EaseInCubic,
        EaseOutCubic,
        EaseInOutCubic,
        EaseInQuart,
        EaseOutQuart,
        EaseInOutQuart,
        EaseInQuint,
        EaseOutQuint,
        EaseInOutQuint,
        EaseInExpo,
        EaseOutExpo,
        EaseInOutExpo,
        EaseInCirc,
        EaseOutCirc,
        EaseInOutCirc,
        EaseInBack,
        EaseOutBack,
        EaseInOutBack,
        EaseInElastic,
        EaseOutElastic,
        EaseInOutElastic,
        EaseInBounce,
        EaseOutBounce,
        EaseInOutBounce,
    ]
};

fn animation(value: &mut Dynamic) -> Result<Animation, Box<EvalAltResult>> {
    take::<Animation>(value, "iced::Animation")
}

/// The animation a shared receiver lends, left in place.
fn read(value: &Dynamic) -> Result<Animation, Box<EvalAltResult>> {
    value
        .read_lock::<Animation>()
        .map(|animation| animation.clone())
        .ok_or_else(|| runtime_error("expects an `iced::Animation`".into()))
}

fn instant(value: &mut Dynamic) -> Result<iced::time::Instant, Box<EvalAltResult>> {
    take::<Instant>(value, "iced::time::Instant").map(|instant| instant.0)
}

/// The animation of a new `bool` or number state.
fn new(state: &mut Dynamic) -> Result<Animation, Box<EvalAltResult>> {
    if let Ok(state) = state.as_bool() {
        return Ok(Animation::Bool(iced::Animation::new(state)));
    }
    Ok(Animation::Float(iced::Animation::new(number(state, "Animation::new")? as f32)))
}

/// `animation` moved to `state` at `at`.
fn go(animation: &mut Animation, state: &mut Dynamic, at: iced::time::Instant) -> Result<(), Box<EvalAltResult>> {
    match animation {
        Animation::Bool(animation) => {
            let state = state
                .as_bool()
                .map_err(|_| runtime_error("`Animation<bool>::go` expects a `bool`".into()))?;
            animation.go_mut(state, at);
        }
        Animation::Float(animation) => animation.go_mut(number(state, "Animation::go")? as f32, at),
    }
    Ok(())
}

/// Apply one builder to either animation.
fn build(
    animation: Animation,
    bool: impl FnOnce(iced::Animation<bool>) -> iced::Animation<bool>,
    float: impl FnOnce(iced::Animation<f32>) -> iced::Animation<f32>,
) -> Animation {
    match animation {
        Animation::Bool(animation) => Animation::Bool(bool(animation)),
        Animation::Float(animation) => Animation::Float(float(animation)),
    }
}

pub(super) fn bind(builder: &mut host::runtime::Builder) {
    // An animation holds no script code: its state's type is its `T`.
    builder
        .bind_type_attestation("iced::Animation", |value, expected| {
            let value = value.read_lock::<Animation>()?;
            let state = match &*value {
                Animation::Bool(animation) => Dynamic::from(animation.value()),
                Animation::Float(animation) => Dynamic::from(f64::from(animation.value())),
            };
            expected.attest(expected.argument_count() == 1 && expected.argument_matches(0, &state))
        })
        .expect("the iced facade must declare generic Animation exactly once");
    builder
        .bind("iced::Animation::new", |args| {
            let [state] = args else {
                return Err(arity("Animation::new", 1, args.len()));
            };
            new(state).map(Dynamic::from)
        })
        .expect("the iced facade must retain Animation::new");
    for name in [
        "very_quick",
        "quick",
        "slow",
        "very_slow",
        "repeat_forever",
        "auto_reverse",
    ] {
        let path = format!("iced::Animation::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [value] = args else {
                    return Err(arity(&path, 1, args.len()));
                };
                let value = animation(value)?;
                Ok(Dynamic::from(match name {
                    "very_quick" => build(value, |a| a.very_quick(), |a| a.very_quick()),
                    "quick" => build(value, |a| a.quick(), |a| a.quick()),
                    "slow" => build(value, |a| a.slow(), |a| a.slow()),
                    "very_slow" => build(value, |a| a.very_slow(), |a| a.very_slow()),
                    "repeat_forever" => build(value, |a| a.repeat_forever(), |a| a.repeat_forever()),
                    _ => build(value, |a| a.auto_reverse(), |a| a.auto_reverse()),
                }))
            })
            .expect("the iced facade must retain every Animation builder");
    }
    builder
        .bind("iced::Animation::easing", |args| {
            let [value, easing] = args else {
                return Err(arity("Animation::easing", 2, args.len()));
            };
            let Easing(easing) = take::<Easing>(easing, "iced::animation::Easing")?;
            Ok(Dynamic::from(build(
                animation(value)?,
                |a| a.easing(easing),
                |a| a.easing(easing),
            )))
        })
        .expect("the iced facade must retain Animation::easing");
    for name in ["duration", "delay"] {
        let path = format!("iced::Animation::{name}");
        builder
            .bind(path.clone(), move |args| {
                let [value, duration] = args else {
                    return Err(arity(&path, 2, args.len()));
                };
                let duration = take::<Duration>(duration, "iced::time::Duration")?;
                let value = animation(value)?;
                Ok(Dynamic::from(if name == "duration" {
                    build(value, |a| a.duration(duration), |a| a.duration(duration))
                } else {
                    build(value, |a| a.delay(duration), |a| a.delay(duration))
                }))
            })
            .expect("the iced facade must retain every Animation timing builder");
    }
    builder
        .bind("iced::Animation::repeat", |args| {
            let [value, count] = args else {
                return Err(arity("Animation::repeat", 2, args.len()));
            };
            let count = count
                .as_int()
                .ok()
                .and_then(|count| u32::try_from(count).ok())
                .ok_or_else(|| runtime_error("`Animation::repeat` expects a `u32`".into()))?;
            Ok(Dynamic::from(build(
                animation(value)?,
                |a| a.repeat(count),
                |a| a.repeat(count),
            )))
        })
        .expect("the iced facade must retain Animation::repeat");
    builder
        .bind("iced::Animation::go", |args| {
            let [value, state, at] = args else {
                return Err(arity("Animation::go", 3, args.len()));
            };
            let mut value = animation(value)?;
            go(&mut value, state, instant(at)?)?;
            Ok(Dynamic::from(value))
        })
        .expect("the iced facade must retain Animation::go");
    builder
        .bind("iced::Animation::go_mut", |args| {
            let [value, state, at] = args else {
                return Err(arity("Animation::go_mut", 3, args.len()));
            };
            let at = instant(at)?;
            let mut moved = animation(value)?;
            go(&mut moved, state, at)?;
            **value = Dynamic::from(moved);
            Ok(Dynamic::UNIT)
        })
        .expect("the iced facade must retain Animation::go_mut");
    builder
        .bind("iced::Animation::is_animating", |args| {
            let [value, at] = args else {
                return Err(arity("Animation::is_animating", 2, args.len()));
            };
            let at = instant(at)?;
            Ok(Dynamic::from(match read(value)? {
                Animation::Bool(animation) => animation.is_animating(at),
                Animation::Float(animation) => animation.is_animating(at),
            }))
        })
        .expect("the iced facade must retain Animation::is_animating");
    builder
        .bind("iced::Animation::value", |args| {
            let [value] = args else {
                return Err(arity("Animation::value", 1, args.len()));
            };
            Ok(match read(value)? {
                Animation::Bool(animation) => Dynamic::from(animation.value()),
                Animation::Float(animation) => Dynamic::from(f64::from(animation.value())),
            })
        })
        .expect("the iced facade must retain Animation::value");
    builder
        .bind("iced::Animation::remaining", |args| {
            let [value, at] = args else {
                return Err(arity("Animation::remaining", 2, args.len()));
            };
            let at = instant(at)?;
            match read(value)? {
                Animation::Bool(animation) => Ok(Dynamic::from(animation.remaining(at))),
                Animation::Float(_) => Err(runtime_error(
                    "`Animation::remaining` is iced's for an `Animation<bool>`".into(),
                )),
            }
        })
        .expect("the iced facade must retain Animation::remaining");
    builder
        .bind("iced::Animation::interpolate", |args| match args {
            [value, at] => {
                let at = instant(at)?;
                match read(value)? {
                    Animation::Float(animation) => {
                        Ok(Dynamic::from(f64::from(animation.interpolate(at))))
                    }
                    Animation::Bool(animation) => {
                        Ok(Dynamic::from(f64::from(animation.interpolate(0.0_f32, 1.0, at))))
                    }
                }
            }
            [value, start, end, at] => {
                let start = number(start, "Animation::interpolate")? as f32;
                let end = number(end, "Animation::interpolate")? as f32;
                let at = instant(at)?;
                match read(value)? {
                    Animation::Bool(animation) => {
                        Ok(Dynamic::from(f64::from(animation.interpolate(start, end, at))))
                    }
                    Animation::Float(_) => Err(runtime_error(
                        "`Animation::interpolate(start, end, at)` is iced's for an `Animation<bool>`"
                            .into(),
                    )),
                }
            }
            _ => Err(arity("Animation::interpolate", 2, args.len())),
        })
        .expect("the iced facade must retain Animation::interpolate");
    for (name, easing) in crate::EASINGS.into_iter().zip(EASINGS) {
        builder
            .bind(format!("iced::animation::Easing::{name}"), move |_| {
                Ok(Dynamic::from(Easing(easing)))
            })
            .expect("the iced facade must retain every Easing value");
    }
}
