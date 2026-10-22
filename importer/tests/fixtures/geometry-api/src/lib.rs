mod shapes {
    pub struct Point {
        pub x: i64,
        pub y: i64,
    }

    pub enum Axis {
        Horizontal,
        Vertical,
    }

    pub const ORIGIN: Point = Point { x: 0, y: 0 };

    pub fn translate(point: Point, x: i64, y: i64) -> Point {
        Point {
            x: point.x + x,
            y: point.y + y,
        }
    }

    impl Point {
        pub const fn new(x: i64, y: i64) -> Self {
            Self { x, y }
        }

        pub fn magnitude_squared(&self) -> i64 {
            self.x * self.x + self.y * self.y
        }
    }
}

pub use shapes::{Axis, ORIGIN, Point, translate};
