//! Colors in the shape of palette's: spaces generic over a standard and a
//! component type, both defaulted, and conversions a program reaches
//! through one blanket impl, as palette's `FromColor` is implemented for
//! every type that is `FromColorUnclamped<T> + Clamp`.

use std::marker::PhantomData;

/// The sRGB standard, as palette's `encoding::Srgb`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Srgb;

/// An RGB color, as palette's `rgb::Rgb<S = Srgb, T = f32>`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rgb<S = Srgb, T = f32> {
    /// Red, from 0 to 1.
    pub red: T,
    /// Green, from 0 to 1.
    pub green: T,
    /// Blue, from 0 to 1.
    pub blue: T,
    /// The standard.
    pub standard: PhantomData<S>,
}

/// An HSL color, as palette's `Hsl<S = Srgb, T = f32>`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hsl<S = Srgb, T = f32> {
    /// Hue, in degrees.
    pub hue: T,
    /// Saturation, from 0 to 1.
    pub saturation: T,
    /// Lightness, from 0 to 1.
    pub lightness: T,
    /// The standard.
    pub standard: PhantomData<S>,
}

pub mod convert {
    //! palette's `convert`: the conversion a space writes, and the one a
    //! program calls, which a blanket impl gives every space.

    /// A conversion that may leave the color out of range, as palette's
    /// `FromColorUnclamped`.
    pub trait FromColorUnclamped<T>: Sized {
        /// `value` in this space.
        fn from_color_unclamped(value: T) -> Self;
    }

    /// Bringing a color into range, as palette's `Clamp`.
    pub trait Clamp {
        /// The color, in range.
        #[must_use]
        fn clamp(self) -> Self;
    }

    /// A conversion into range, as palette's `FromColor`.
    pub trait FromColor<T>: Sized {
        /// `value` in this space, in range.
        fn from_color(value: T) -> Self;
    }

    impl<T, U> FromColor<T> for U
    where
        U: FromColorUnclamped<T> + Clamp,
    {
        fn from_color(value: T) -> Self {
            U::from_color_unclamped(value).clamp()
        }
    }
}

use convert::{Clamp, FromColorUnclamped};

impl<S> FromColorUnclamped<Rgb<S, f32>> for Hsl<S, f32> {
    fn from_color_unclamped(rgb: Rgb<S, f32>) -> Self {
        let max = rgb.red.max(rgb.green).max(rgb.blue);
        let min = rgb.red.min(rgb.green).min(rgb.blue);
        let lightness = (max + min) / 2.0;
        let delta = max - min;
        let saturation = if delta == 0.0 {
            0.0
        } else {
            delta / (1.0 - (2.0 * lightness - 1.0).abs())
        };
        let hue = if delta == 0.0 {
            0.0
        } else if max == rgb.red {
            60.0 * ((rgb.green - rgb.blue) / delta)
        } else if max == rgb.green {
            60.0 * ((rgb.blue - rgb.red) / delta + 2.0)
        } else {
            60.0 * ((rgb.red - rgb.green) / delta + 4.0)
        };
        Hsl {
            hue,
            saturation,
            lightness,
            standard: rgb.standard,
        }
    }
}

impl<S> FromColorUnclamped<Hsl<S, f32>> for Rgb<S, f32> {
    fn from_color_unclamped(hsl: Hsl<S, f32>) -> Self {
        let chroma = (1.0 - (2.0 * hsl.lightness - 1.0).abs()) * hsl.saturation;
        let side = hsl.hue.rem_euclid(360.0) / 60.0;
        let second = chroma * (1.0 - (side % 2.0 - 1.0).abs());
        let (red, green, blue) = match side as u32 {
            0 => (chroma, second, 0.0),
            1 => (second, chroma, 0.0),
            2 => (0.0, chroma, second),
            3 => (0.0, second, chroma),
            4 => (second, 0.0, chroma),
            _ => (chroma, 0.0, second),
        };
        let lift = hsl.lightness - chroma / 2.0;
        Rgb {
            red: red + lift,
            green: green + lift,
            blue: blue + lift,
            standard: hsl.standard,
        }
    }
}

// Each space converts from itself too, as palette's derive writes, so the
// blanket impl applies to each space through several impls of its bound.
impl<S> FromColorUnclamped<Hsl<S, f32>> for Hsl<S, f32> {
    fn from_color_unclamped(hsl: Hsl<S, f32>) -> Self {
        hsl
    }
}

impl<S> FromColorUnclamped<Rgb<S, f32>> for Rgb<S, f32> {
    fn from_color_unclamped(rgb: Rgb<S, f32>) -> Self {
        rgb
    }
}

impl<S> Clamp for Hsl<S, f32> {
    fn clamp(self) -> Self {
        Hsl {
            hue: self.hue.rem_euclid(360.0),
            saturation: self.saturation.clamp(0.0, 1.0),
            lightness: self.lightness.clamp(0.0, 1.0),
            standard: self.standard,
        }
    }
}

impl<S> Clamp for Rgb<S, f32> {
    fn clamp(self) -> Self {
        Rgb {
            red: self.red.clamp(0.0, 1.0),
            green: self.green.clamp(0.0, 1.0),
            blue: self.blue.clamp(0.0, 1.0),
            standard: self.standard,
        }
    }
}
