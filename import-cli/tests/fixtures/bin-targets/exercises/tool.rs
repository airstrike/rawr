// rawr
//! A `[[bin]]` outside `src/`: it greets through the host-only dependency,
//! and panics when asked to.

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    let greeting = gated::greeting("tool");
    println!("{greeting}");
    if std::env::args().nth(1).as_deref() == Some("panic") {
        panic!("asked to panic");
    }
}
