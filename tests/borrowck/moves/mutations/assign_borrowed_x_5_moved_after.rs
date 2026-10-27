fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    println!("{}", *r + x);
    __take(x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
