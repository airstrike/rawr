fn main() {
    let mut x = 1;
    let r = &x;
    if __c() {
        __take(x);
        panic!();
    }
    x = 2;
    println!("{}", *r + x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
