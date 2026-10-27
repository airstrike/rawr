fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    if __c() {
        __take(x);
    }
    println!("{}", *r + x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
