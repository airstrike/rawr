//! Native `main`, as inboard's: it keeps the marked home screen's state
//! and asks it for its greeting each tick, until the file `ISLANDS_STOP`
//! names exists.
mod home;
mod style;
mod theme;

fn main() {
    let stop = std::env::var_os("ISLANDS_STOP").expect("ISLANDS_STOP names the stop file");
    let mut home = home::State::new();
    for tick in 1..3000 {
        if std::path::Path::new(&stop).exists() {
            break;
        }
        home.tick();
        println!("tick {tick} at {} says {}", home.count(), home.greeting());
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
