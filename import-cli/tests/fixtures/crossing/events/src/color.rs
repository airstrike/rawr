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

/// The D65 white point, as palette's `white_point::D65`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct D65;

/// An RGB space, as palette's `rgb::RgbSpace`: its white point.
pub trait RgbSpace {
    /// The white point of the space.
    type WhitePoint;
}

/// An RGB standard, as palette's `rgb::RgbStandard`: its space.
pub trait RgbStandard {
    /// The space of the standard.
    type Space: RgbSpace;
}

impl RgbSpace for Srgb {
    type WhitePoint = D65;
}

impl RgbStandard for Srgb {
    type Space = Srgb;
}

/// A CIE L*a*b* color, as palette's `Lab<Wp = D65, T = f32>`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lab<Wp = D65, T = f32> {
    /// Lightness, from 0 to 100.
    pub l: T,
    /// Green to red.
    pub a: T,
    /// Blue to yellow.
    pub b: T,
    /// The white point.
    pub white_point: PhantomData<Wp>,
}

impl<Wp> Lab<Wp, f32> {
    /// A color of `l`, `a` and `b`, as palette's `Lab::new`.
    pub const fn new(l: f32, a: f32, b: f32) -> Self {
        Lab {
            l,
            a,
            b,
            white_point: PhantomData,
        }
    }
}

// An RGB color converts from Lab at its standard's white point, which
// palette's derive writes as a projection through the standard's space.
impl<S> FromColorUnclamped<Lab<<S::Space as RgbSpace>::WhitePoint, f32>> for Rgb<S, f32>
where
    S: RgbStandard,
{
    fn from_color_unclamped(lab: Lab<<S::Space as RgbSpace>::WhitePoint, f32>) -> Self {
        let lightness = lab.l / 100.0;
        Rgb {
            red: lightness + lab.a / 200.0,
            green: lightness - lab.a / 400.0 + lab.b / 400.0,
            blue: lightness - lab.b / 200.0,
            standard: PhantomData,
        }
    }
}

impl<Wp> FromColorUnclamped<Lab<Wp, f32>> for Lab<Wp, f32> {
    fn from_color_unclamped(lab: Lab<Wp, f32>) -> Self {
        lab
    }
}

impl<Wp> Clamp for Lab<Wp, f32> {
    fn clamp(self) -> Self {
        Lab {
            l: self.l.clamp(0.0, 100.0),
            ..self
        }
    }
}

/// Shifting the hue by an amount, as palette's `ShiftHue`, whose amount is
/// the trait's own associated type that each impl sets to the component.
pub trait ShiftHue {
    /// The type of the hue modifier.
    type Scalar;

    /// The color with its hue shifted by `amount`.
    #[must_use]
    fn shift_hue(self, amount: Self::Scalar) -> Self;
}

impl<S, T> ShiftHue for Hsl<S, T>
where
    T: std::ops::Add<Output = T>,
{
    type Scalar = T;

    fn shift_hue(mut self, amount: Self::Scalar) -> Self {
        self.hue = self.hue + amount;
        self
    }
}

/// Lightening by a factor, as palette's `Lighten`.
pub trait Lighten {
    /// The type of the lighten modifier.
    type Scalar;

    /// The color lightened by `factor`.
    #[must_use]
    fn lighten(self, factor: Self::Scalar) -> Self;
}

impl<S> Lighten for Hsl<S, f32> {
    type Scalar = f32;

    fn lighten(self, factor: Self::Scalar) -> Self {
        Hsl {
            lightness: self.lightness + (1.0 - self.lightness) * factor,
            ..self
        }
    }
}

/// Darkening by a factor, as palette's `Darken`, which every `Lighten`
/// type implements through one blanket impl whose `Scalar` is the
/// `Lighten` impl's.
pub trait Darken {
    /// The type of the darken modifier.
    type Scalar;

    /// The color darkened by `factor`.
    #[must_use]
    fn darken(self, factor: Self::Scalar) -> Self;
}

impl<T> Darken for T
where
    T: Lighten,
    T::Scalar: std::ops::Neg<Output = T::Scalar>,
{
    type Scalar = T::Scalar;

    fn darken(self, factor: Self::Scalar) -> Self {
        self.lighten(-factor)
    }
}
