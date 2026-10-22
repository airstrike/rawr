// rawr
// Time spans with chrono. Rawr imports chrono from this crate's
// Cargo.toml, with no rawr.toml.
use chrono::TimeDelta;

fn main() {
    let lap = TimeDelta::seconds(95);
    let race = TimeDelta::minutes(42);
    println!("lap: {} minutes, {} seconds", lap.num_minutes(), lap.num_seconds());
    println!("race: {} seconds", race.num_seconds());
    println!("hours: {}", TimeDelta::hours(3).num_minutes() / 60);
}
