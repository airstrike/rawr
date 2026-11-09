// rawr
// A program for wasm32 over a crate whose build script runs on the host.

fn main() {
    println!("built for {}", facts::arch());
}
