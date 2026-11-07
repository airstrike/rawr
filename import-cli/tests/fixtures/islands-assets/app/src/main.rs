//! Native `main`: prints what the package read at compile time and what
//! its features enabled, then counts ticks through the marked module
//! `physics` while the file `ISLANDS_STOP` names is absent.
mod assets;
mod features;
mod physics;

fn main() {
    println!("logo {}", String::from_utf8_lossy(assets::LOGO));
    println!("icon {}", assets::ICON);
    println!("shared {}", assets::SHARED);
    println!("icons {}", assets::ICONS.join(","));
    println!("greet {}", features::greeting());
    println!("loud {}", features::loud());
    println!("never {}", features::never());
    println!("marked {}", physics::greeting());
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
