// rawr
//! The part of the program being worked on. Under `rawr run` this file is
//! interpreted, and a save takes effect on the next tick. `cargo build`
//! compiles it like any other module.

/// Downward acceleration, in rows per second squared.
const GRAVITY: f64 = 9.8;
/// The share of its speed the ball keeps when it hits the floor.
const BOUNCE: f64 = 0.8;

/// A ball moving over a track and bouncing on the floor.
#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub x: f64,
    pub height: f64,
    pub across: f64,
    pub up: f64,
}

impl Ball {
    /// A ball at `x`, dropped from `height`.
    pub fn new(x: f64, height: f64) -> Ball {
        Ball {
            x,
            height,
            across: 6.0,
            up: 0.0,
        }
    }

    /// Move the ball by `step` seconds on a track `width` wide.
    pub fn advance(&mut self, step: f64, width: f64) {
        self.up -= GRAVITY * step;
        self.height += self.up * step;
        if self.height < 0.0 {
            self.height = 0.0;
            self.up = -self.up * BOUNCE;
        }
        self.x += self.across * step;
        if self.x < 0.0 || self.x > width - 1.0 {
            self.across = -self.across;
            self.x = self.x.clamp(0.0, width - 1.0);
        }
    }
}

/// One line of the track with the ball on it, and its height.
pub fn render(ball: &Ball, width: usize) -> String {
    let column = ball.x.round() as usize;
    let mut line = String::new();
    for index in 0..width {
        if index == column {
            line.push('o');
        } else {
            line.push('.');
        }
    }
    format!("|{line}| height {:>5.2}", ball.height)
}

/// What the run ended with.
pub fn summary(ball: &Ball, ticks: u64) -> String {
    format!("{ticks} ticks, the ball ended at {:.1}", ball.x)
}
