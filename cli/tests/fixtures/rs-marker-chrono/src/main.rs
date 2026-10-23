// rawr
// Time spans and the clock with chrono. Rawr imports chrono from this
// crate's Cargo.toml.
use chrono::{Datelike, Duration, Local, TimeDelta};

fn main() {
    let lap = TimeDelta::seconds(95);
    let race = TimeDelta::minutes(42);
    println!("lap: {} minutes, {} seconds", lap.num_minutes(), lap.num_seconds());
    println!("race: {} seconds", race.num_seconds());
    println!("hours: {}", TimeDelta::hours(3).num_minutes() / 60);

    // `chrono::Duration` is a type alias of `TimeDelta`.
    println!("90 seconds: {} minutes", Duration::seconds(90).num_minutes());

    // `Local::now()` is a `DateTime<Local>`, and `year` is a method of the
    // `Datelike` trait.
    let now = Local::now();
    println!("this year or later: {}", now.year() >= 2026);
}
