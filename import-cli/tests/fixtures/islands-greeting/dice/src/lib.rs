//! A stand-in for `rand` 0.9 with its shapes: `rng()` gives a `ThreadRng`
//! by value, and `random_range` is the `Rng` trait's `&mut self` method,
//! which a blanket impl gives every `RngCore`. Its draws are fixed.

pub trait RngCore {
    fn next_u32(&mut self) -> u32;
}

/// A type a range of which can be sampled.
pub trait SampleUniform: Sized {
    fn pick(low: Self, high: Self, draw: u32) -> Self;
}

impl SampleUniform for usize {
    fn pick(low: Self, high: Self, draw: u32) -> Self {
        low + usize::try_from(draw).unwrap_or(0) % (high - low)
    }
}

impl SampleUniform for u32 {
    fn pick(low: Self, high: Self, draw: u32) -> Self {
        low + draw % (high - low)
    }
}

pub trait SampleRange<T> {
    fn sample_single<R: RngCore + ?Sized>(self, rng: &mut R) -> T;
}

impl<T: SampleUniform> SampleRange<T> for std::ops::Range<T> {
    fn sample_single<R: RngCore + ?Sized>(self, rng: &mut R) -> T {
        T::pick(self.start, self.end, rng.next_u32())
    }
}

pub trait Rng: RngCore {
    fn random_range<T, R>(&mut self, range: R) -> T
    where
        T: SampleUniform,
        R: SampleRange<T>,
    {
        range.sample_single(self)
    }
}

impl<R: RngCore + ?Sized> Rng for R {}

/// The thread's generator, as `rand::rngs::ThreadRng`.
#[derive(Debug, Clone)]
pub struct ThreadRng {
    state: u32,
}

impl RngCore for ThreadRng {
    fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.state >> 8
    }
}

pub fn rng() -> ThreadRng {
    ThreadRng { state: 7 }
}

pub mod prelude {
    pub use crate::{rng, Rng, RngCore, ThreadRng};
}
