// rawr
// The crate's `Halve` has its own `into`, beside `Into::into` of the
// crate's `From<Gauge>` for `f32` and `f64`. `Halve` is not in scope, so
// `gauge.into()` is `Into::into`, whose target picks the impl, as rustc
// selects it.

use events::controls::Gauge;

fn main() {
    let gauge = Gauge(1.5);
    let wide: f64 = gauge.into();
    let narrow: f32 = gauge.into();
    println!("{wide} {narrow}");
}
