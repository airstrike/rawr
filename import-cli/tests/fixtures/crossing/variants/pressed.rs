// rawr
use events::matched::{self, Press};

fn main() {
    let Press::Down(door) = matched::press("gate");
    println!("down {}", door.close());
    let matched::Press::Down(ref door) = matched::press("vent");
    println!("held {}", door.name());
}
