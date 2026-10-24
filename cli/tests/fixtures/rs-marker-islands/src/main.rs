//! Native Rust: `main` calls the marked module `physics` each tick.
mod physics;
mod units;

fn main() {
    let mut ball = physics::Ball::new(units::START);
    for _ in 0..3 {
        ball.advance(0.5);
        println!("{}", physics::describe(&ball));
    }
}
