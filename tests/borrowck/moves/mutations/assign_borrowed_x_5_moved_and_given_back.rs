fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __m = x;
    x = __m;
    println!("{}", *r + x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
