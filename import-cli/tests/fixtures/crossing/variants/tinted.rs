// rawr
// iced's `color!(0xff0000)` and `border::color(c).width(1)`: the crate's
// module `tint`, its macro `tint!` and the function `outline::tint` share
// a name in their own namespaces. With `EVENTS_STOP` it shows its line
// until that file exists, so a save can change what it shows.

use events::{outline, tint, Tint};
use std::path::Path;
use std::time::Duration;

fn shown() -> String {
    let red: Tint = tint!(0xff0000);
    let edge = outline::tint(red).width(1);
    format!("{red:?} {edge:?}")
}

fn main() {
    let stop = std::env::var_os("EVENTS_STOP");
    let mut step = 0;
    loop {
        println!("{step} {}", shown());
        match &stop {
            Some(stop) if !Path::new(stop).exists() => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => break,
        }
        step += 1;
    }
}
