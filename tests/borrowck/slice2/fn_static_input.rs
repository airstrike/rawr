fn keep(_x: &'static i32) {}

fn main() {
    let x = 5;
    keep(&x);
    println!("{x}");
}
