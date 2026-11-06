// rawr
use natives::angle::Turn;

struct Dial {
    turn: Turn,
}

impl Dial {
    fn turn(&self) -> Turn {
        self.turn
    }
}

fn main() {
    let a = Turn(0.7);
    println!("{:?} {:?} {:?}", 2.0 * a, a * 2.0, a * a);
    let mut dial = Dial { turn: a };
    // iced's ferris: `(rotation + step) % (2.0 * Radians::PI)`.
    dial.turn = (dial.turn() + Turn(0.1)) % (2.0 * Turn::HALF);
    println!("{:?}", dial.turn);
    println!("{:?} {:?}", 10.0 - a, 3 / Turn(2.0));
    let x: f32 = 1.5;
    println!("{:?}", x * a);
}
