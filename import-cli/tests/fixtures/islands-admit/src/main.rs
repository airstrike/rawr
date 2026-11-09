//! Native `main` that goes idle as an event loop does: it calls the marked
//! module `physics` once, then makes no call into marked code until the
//! file `IDLE_GO` names exists, calls it once more, and stays until the
//! file `IDLE_STOP` names exists.
mod physics;

fn main() {
    println!("{}", physics::label());
    after("IDLE_GO");
    println!("{}", physics::label());
    after("IDLE_STOP");
}

/// Once the file `variable` names exists.
fn after(variable: &str) {
    let file = std::env::var_os(variable).expect("the test names the file");
    while !std::path::Path::new(&file).exists() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
