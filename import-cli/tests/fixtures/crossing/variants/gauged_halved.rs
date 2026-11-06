// rawr
// The crate's `Halve` is in scope, so `gauge.into()` finds its `into` and
// `Into::into` both: rustc's E0034, whatever the target.

use events::controls::{Gauge, Halve};

fn main() {
    let gauge = Gauge(1.5);
    let wide: f64 = gauge.into();
    println!("{wide}");
}
