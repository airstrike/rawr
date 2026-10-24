//! Native `main`: counts ticks through the marked module `physics` until
//! the file `ISLANDS_STOP` names exists.
mod physics;

fn main() {
    let stop = std::env::var_os("ISLANDS_STOP");
    let mut counter = physics::Counter::new();
    for _ in 0..3000 {
        if stop
            .as_ref()
            .is_some_and(|path| std::path::Path::new(path).exists())
        {
            break;
        }
        counter.step();
        println!("{} {}", counter.ticks, physics::label(&counter));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
