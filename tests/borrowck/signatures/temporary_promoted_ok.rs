fn main() {
    let x: &'static i32 = &5;
    let y = &[1, 2, 3];
    println!("{x} {y:?}");
}
