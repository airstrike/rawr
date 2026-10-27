fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    for __i in 0..2 {
        let __m = x;
        x = __m;
    }
    x = 2;
    println!("{}", x + y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
