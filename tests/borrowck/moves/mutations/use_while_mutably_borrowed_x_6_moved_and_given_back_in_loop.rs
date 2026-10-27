fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    for __i in 0..2 {
        let __m = x;
        x = __m;
    }
    println!("{} {}", x, y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
