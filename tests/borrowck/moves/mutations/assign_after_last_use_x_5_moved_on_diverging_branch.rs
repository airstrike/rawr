fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    if __c() {
        __take(x);
        panic!();
    }
    x = 2;
    println!("{}", x + y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
