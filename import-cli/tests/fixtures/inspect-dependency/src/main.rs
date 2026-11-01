// rawr
// A marked `main` that hands control to a native loop, as an iced app
// does, which calls the script function `update` back each tick.
use pump::{Count, Sealed};

#[derive(Debug)]
struct State {
    count: Count,
    label: String,
}

fn update(tick: i64) -> i64 {
    let state = State {
        count: Count::at(tick),
        label: String::from("ticks"),
    };
    let sealed = Sealed::new();
    let _ = (&state, &sealed);
    tick
}

fn main() {
    pump::run(update);
}
