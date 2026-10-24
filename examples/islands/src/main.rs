//! An ordinary program: `main` is native Rust, and `physics.rs`, which
//! starts with `// rawr`, is the part being worked on. `cargo run` builds
//! both natively. `rawr run` interprets `physics.rs` and picks up each save
//! on the next tick.

mod physics;

use std::time::{Duration, Instant};

/// The width of the track the ball bounces along, in characters.
const WIDTH: usize = 40;
/// Ticks per second.
const RATE: u64 = 4;

fn main() {
    // `ISLANDS_SECONDS=0` runs until Ctrl-C.
    let seconds = std::env::var("ISLANDS_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20);
    let mut ball = physics::Ball::new(WIDTH as f64 / 2.0, 8.0);
    let start = Instant::now();
    let step = 1.0 / RATE as f64;
    let mut tick = 0_u64;
    while seconds == 0 || start.elapsed() < Duration::from_secs(seconds) {
        ball.advance(step, WIDTH as f64);
        println!("{tick:>4} {}", physics::render(&ball, WIDTH));
        tick += 1;
        std::thread::sleep(Duration::from_millis(1000 / RATE));
    }
    println!("{}", physics::summary(&ball, tick));
}
