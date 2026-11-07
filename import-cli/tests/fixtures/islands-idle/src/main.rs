//! Native `main` that goes idle as an event loop does: it calls the marked
//! module `physics` once, then makes no call into marked code until the
//! file `IDLE_GO` names exists, and then calls it once more.
mod physics;

fn main() {
    println!("pid {}", std::process::id());
    println!("{}", physics::label());
    let go = std::env::var_os("IDLE_GO").expect("IDLE_GO names a file");
    while !std::path::Path::new(&go).exists() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    println!("{}", physics::label());
}
