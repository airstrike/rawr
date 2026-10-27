//! A dependency whose items have the shapes upstream iced examples reach
//! (T204): a struct with a `PhantomData` field and defaulted parameters,
//! as palette's `Rgb` is; references to tuples and a slice, as voronator's
//! `from_tuple` takes; and a `&[u8]`, as image's `save_buffer` takes.

use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Srgb;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb<S = Srgb, T = f32> {
    pub red: T,
    pub green: T,
    pub blue: T,
    pub standard: PhantomData<S>,
}

impl<S, T: Default> Default for Rgb<S, T> {
    fn default() -> Self {
        Self {
            red: T::default(),
            green: T::default(),
            blue: T::default(),
            standard: PhantomData,
        }
    }
}

/// How many of `points` lie inside the box from `min` to `max`.
pub fn inside(min: &(f64, f64), max: &(f64, f64), points: &[(f64, f64)]) -> Option<usize> {
    if min.0 > max.0 || min.1 > max.1 {
        return None;
    }
    Some(
        points
            .iter()
            .filter(|(x, y)| (min.0..=max.0).contains(x) && (min.1..=max.1).contains(y))
            .count(),
    )
}

/// Zeroes `bytes`. Rawr doesn't bind a `&mut [u8]` yet, and its
/// diagnostic says so.
pub fn clear(bytes: &mut [u8]) {
    bytes.fill(0);
}

/// The sum of `bytes`, each weighted by its position.
pub fn checksum(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .enumerate()
        .map(|(index, byte)| (index as u64 + 1) * u64::from(*byte))
        .sum()
}
