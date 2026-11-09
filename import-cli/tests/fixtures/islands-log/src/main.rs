//! Native `main`, as inboard's: it sets `log`'s level, then calls the
//! marked `inbox` module, which compares `tracing`'s and `log`'s levels.
mod workspace;

use workspace::inbox;

fn main() {
    log::set_max_level(log::LevelFilter::Info);
    println!("{}", inbox::levels());
    println!("{}", inbox::reexported());
    println!("{}", inbox::here());
    println!("{}", inbox::name());
    println!("{}", inbox::traced());
    println!("{}", inbox::modules());
    println!("{}", inbox::crossing());
}
