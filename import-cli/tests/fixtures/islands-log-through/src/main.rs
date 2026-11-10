//! Native `main`, as inboard's: it sets its `log` crate's subscriber, then
//! calls the marked `notice` module, which logs through that crate.
mod workspace;

use workspace::notice;

fn main() {
    log::init();
    println!("{}", notice::fallback());
}
