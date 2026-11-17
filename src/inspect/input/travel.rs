//! The path the agent cursor travels from where it rests to where the next
//! pointer event goes, so the window shows it moving there as a hand would
//! move a mouse, not appearing at the destination.
//!
//! A path is a quadratic Bézier curve: its control point sits off the
//! straight line between the two points, toward the window's middle, so
//! the curve stays inside the window. The control point's offset grows
//! faster than the distance, so a short hop is nearly straight and a long
//! move bends. A held button's path is straight, so a drag goes where it
//! was asked to. The cursor eases in and out along it over about a fifth
//! of a second, less for a short hop.

use std::time::Duration;

use super::Point;

/// How long the longest move takes.
const LONGEST: Duration = Duration::from_millis(210);

/// How long a move of a point or two takes.
const SHORTEST: Duration = Duration::from_millis(120);

/// The distance, in points, from which a move takes [`LONGEST`] and bends
/// its most.
const FAR: f64 = 400.0;

/// How far the control point of a move of [`FAR`] or more sits off its
/// line, as a part of the move's length: the curve's middle sits half as
/// far off.
const BEND: f64 = 0.2;

/// About one frame at 60 Hz.
const FRAME: Duration = Duration::from_micros(16_667);

/// The farthest a held button moves between two of its events, in points.
const STRIDE: f64 = 8.0;

/// The most points a path is shown at.
const MOST: u32 = 60;

/// A path from one point to another, and how long the cursor takes on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Travel {
    from: Point,
    control: Point,
    to: Point,
    steps: u32,
    pause: Duration,
}

impl Travel {
    /// A path from `from` to `to` in a window `width` by `height` points,
    /// bending toward its middle.
    pub(super) fn curved(from: Point, to: Point, width: f64, height: f64) -> Self {
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let distance = dx.hypot(dy);
        let middle = Point {
            x: (from.x + to.x) / 2.0,
            y: (from.y + to.y) / 2.0,
        };
        if distance == 0.0 {
            return Self::timed(from, middle, to, distance, 1);
        }
        let offset = distance * BEND * (distance / FAR).min(1.0);
        // The line's normal, turned to the side the window's middle is on.
        let (mut nx, mut ny) = (-dy / distance, dx / distance);
        if nx * (width / 2.0 - middle.x) + ny * (height / 2.0 - middle.y) < 0.0 {
            (nx, ny) = (-nx, -ny);
        }
        // The curve lies between its ends and its control point, so a
        // control point inside the window keeps all of it there.
        let control = Point {
            x: (middle.x + nx * offset).clamp(0.0, width.max(0.0)),
            y: (middle.y + ny * offset).clamp(0.0, height.max(0.0)),
        };
        Self::timed(from, control, to, distance, 1)
    }

    /// A straight path from `from` to `to`, shown at least every
    /// [`STRIDE`] points: an app that follows a held button sees no gap
    /// wider.
    pub(super) fn straight(from: Point, to: Point) -> Self {
        let middle = Point {
            x: (from.x + to.x) / 2.0,
            y: (from.y + to.y) / 2.0,
        };
        let distance = (to.x - from.x).hypot(to.y - from.y);
        let strides = (distance / STRIDE).ceil().min(f64::from(MOST)) as u32;
        Self::timed(from, middle, to, distance, strides)
    }

    /// The path through `control`, shown once a frame and at `least`
    /// points.
    fn timed(from: Point, control: Point, to: Point, distance: f64, least: u32) -> Self {
        let near = (distance / FAR).min(1.0);
        let duration = SHORTEST + (LONGEST - SHORTEST).mul_f64(near);
        let frames = (duration.as_secs_f64() / FRAME.as_secs_f64()).ceil() as u32;
        let steps = if distance < 1.0 {
            1
        } else {
            frames.max(least).min(MOST)
        };
        Self {
            from,
            control,
            to,
            steps,
            pause: duration / steps,
        }
    }

    /// The points the cursor is shown at, in order, the last of them the
    /// destination; one alone for a hop under a point.
    pub(super) fn points(self) -> impl Iterator<Item = Point> {
        (1..=self.steps).map(move |step| {
            if step == self.steps {
                return self.to;
            }
            let t = ease(f64::from(step) / f64::from(self.steps));
            let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t, t * t);
            Point {
                x: a * self.from.x + b * self.control.x + c * self.to.x,
                y: a * self.from.y + b * self.control.y + c * self.to.y,
            }
        })
    }

    /// How long the cursor rests at each point before the next.
    pub(super) fn pause(self) -> Duration {
        if self.steps == 1 {
            Duration::ZERO
        } else {
            self.pause
        }
    }
}

/// A cubic ease in and out: slow at both ends, fastest in the middle.
fn ease(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (2.0 - 2.0 * t).powi(3) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_move_bends_inside_the_window_eases_and_lands_on_its_point() {
        let from = Point { x: 20.0, y: 20.0 };
        let to = Point { x: 420.0, y: 20.0 };
        let travel = Travel::curved(from, to, 500.0, 300.0);
        let points: Vec<Point> = travel.points().collect();
        assert_eq!(points.last(), Some(&to));
        // The window's middle is below the line, so the curve bends down
        // and never leaves the window.
        let lowest = points.iter().map(|point| point.y).fold(0.0, f64::max);
        assert!(lowest > 50.0, "{points:?}");
        assert!(points.iter().all(|point| (0.0..=300.0).contains(&point.y)));
        // Slow at the start, fast in the middle.
        let step = |index: usize| {
            let (a, b) = (points[index], points[index + 1]);
            (b.x - a.x).hypot(b.y - a.y)
        };
        assert!(step(0) < step(points.len() / 2));
        let total = travel.pause() * points.len() as u32;
        assert!(total >= LONGEST - FRAME && total <= LONGEST, "{total:?}");
        // A short hop is nearly straight and quicker.
        let hop = Travel::curved(from, Point { x: 40.0, y: 20.0 }, 500.0, 300.0);
        assert!(hop.points().all(|point| (point.y - 20.0).abs() < 0.5));
        assert!(hop.pause() * hop.points().count() as u32 <= SHORTEST + FRAME);
    }
}
