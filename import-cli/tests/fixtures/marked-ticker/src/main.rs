// rawr
// A marked `main` that hands control to a native loop, which calls the
// script function `step` back each tick.

struct Scale {
    factor: i64,
}

fn step(tick: i64) -> String {
    let scale = Scale { factor: 1 };
    format!("{tick} value {}", tick * scale.factor)
}

fn main() {
    ticker::run(step, 3000);
}
