// rawr
// A program for the target the package's `.cargo/config.toml` names.

fn main() {
    println!("built for {}", facts::arch());
}
