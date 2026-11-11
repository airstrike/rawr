// rawr
use std::ops::AddAssign;

use natives::angle::Turn;

/// A program's own type on the right of a number's `+=`.
struct Steps(f32);

impl AddAssign<Steps> for f32 {
    fn add_assign(&mut self, rhs: Steps) {
        *self += rhs.0 * 10.0;
    }
}

struct Dial {
    turn: f32,
}

fn main() {
    let mut y: f32 = 1.0;
    y += Turn(0.25);
    println!("{y}");
    let mut dial = Dial { turn: 0.5 };
    dial.turn += Turn(1.0);
    println!("{}", dial.turn);
    y += Steps(0.5);
    println!("{y}");
}
