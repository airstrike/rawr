fn main() {
    let mut x = 1;
    let r = &x;
    let __m = x;
    x = __m;
    x = 2;
    println!("{}", *r + x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
