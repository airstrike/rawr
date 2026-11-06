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

/// An area, as iced's `Rectangle`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rectangle<T = f32> {
    /// Its left edge.
    pub x: T,
    /// Its top edge.
    pub y: T,
    /// Across.
    pub width: T,
    /// Down.
    pub height: T,
}

/// Where the mouse is, as iced's `mouse::Cursor`: `Copy`, and its methods
/// take it by value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cursor {
    /// Over the window, at a point.
    Available(Point),
    /// Outside it.
    Unavailable,
}

impl Cursor {
    /// The cursor's point from the corner of `bounds`, when it is over
    /// them, as iced's `Cursor::position_in`.
    pub fn position_in(self, bounds: Rectangle) -> Option<Point> {
        match self {
            Self::Available(point)
                if point.x >= bounds.x
                    && point.y >= bounds.y
                    && point.x <= bounds.x + bounds.width
                    && point.y <= bounds.y + bounds.height =>
            {
                Some(point - Vector::new(bounds.x, bounds.y))
            }
            _ => None,
        }
    }
}

/// What `update` gives for `cursor`, as a canvas program's `update` is
/// handed the cursor by native code.
pub fn hover<T>(cursor: Cursor, update: impl Fn(Cursor) -> Option<T>) -> Option<T> {
    update(cursor)
}
