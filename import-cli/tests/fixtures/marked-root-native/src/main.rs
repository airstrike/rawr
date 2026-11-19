// rawr
//! A marked `main` whose native module calls back into it: `tally::line`
//! names the root's private `scale` and builds its `Reading`. Counts
//! `ISLANDS_TICKS` ticks, or until the file `ISLANDS_STOP` names exists.
mod tally;

const SCALE: i64 = 1;

struct Reading {
    tick: i64,
    value: i64,
}

fn scale(tick: i64) -> i64 {
    tick * SCALE
}

fn main() {
    for tick in 1..=tally::ticks() {
        if tally::stopped() {
            break;
        }
        println!("{}", tally::line(tick));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
