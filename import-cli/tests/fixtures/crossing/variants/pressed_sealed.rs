// rawr
use events::matched::{self, Sealed};

fn main() {
    let Sealed::Down(door) = matched::sealed("gate");
    println!("down {}", door.close());
}
