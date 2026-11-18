// rawr
use natives::dial::{distance, Dial};

const STEPS: u32 = 2;

fn main() {
    let mut dial = Dial::<3>::default();
    println!("{} {} {}", dial.turn(), dial.turn(), dial.turn());
    let mut other: Dial<{ STEPS * 2 + 1 }> = Dial::new(3);
    println!("{} {} {:?}", other.turn(), distance(&other), other);
}
