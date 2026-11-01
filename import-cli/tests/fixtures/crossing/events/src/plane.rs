//! Points and vectors in the shape of iced's: generic over their
//! coordinate, defaulted to `f32`, with operator impls whose `Output`
//! differs from `Self`. `Point - Point` is a `Vector` and `Point - Vector`
//! a `Point`, two impls of one generic trait for one type.

use std::ops::{Add, Mul, Sub};

/// A position, as iced's `Point`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point<T = f32> {
    /// Across.
    pub x: T,
    /// Down.
    pub y: T,
}

/// A displacement, as iced's `Vector`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector<T = f32> {
    /// Across.
    pub x: T,
    /// Down.
    pub y: T,
}

impl<T> Point<T> {
    /// The point at `x` and `y`.
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T> Vector<T> {
    /// The vector of `x` and `y`.
    pub const fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl<T: Sub<Output = T>> Sub<Point<T>> for Point<T> {
    type Output = Vector<T>;

    fn sub(self, point: Point<T>) -> Vector<T> {
        Vector::new(self.x - point.x, self.y - point.y)
    }
}

impl<T: Sub<Output = T>> Sub<Vector<T>> for Point<T> {
    type Output = Self;

    fn sub(self, vector: Vector<T>) -> Self {
        Self::new(self.x - vector.x, self.y - vector.y)
    }
}

impl<T: Add<Output = T>> Add<Vector<T>> for Point<T> {
    type Output = Self;

    fn add(self, vector: Vector<T>) -> Self {
        Self::new(self.x + vector.x, self.y + vector.y)
    }
}

impl<T: Add<Output = T>> Add for Vector<T> {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl<T: Mul<Output = T> + Copy> Mul<T> for Vector<T> {
    type Output = Self;

    fn mul(self, scale: T) -> Self {
        Self::new(self.x * scale, self.y * scale)
    }
}
