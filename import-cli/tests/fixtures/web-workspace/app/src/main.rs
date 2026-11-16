// rawr
fn main() {
    let mut buffer = digits::Buffer::new();
    println!("digits: {}", buffer.format(1234));
    println!("loud: {}", cfg!(feature = "loud"));
    println!("wasm32: {}", cfg!(target_arch = "wasm32"));
}
