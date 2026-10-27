fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    let __m = { x };
    println!("{} {}", x, y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
