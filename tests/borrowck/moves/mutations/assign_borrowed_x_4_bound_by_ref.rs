fn main() {
    let mut x = 1;
    let r = &x;
    let ref __v = x;
    x = 2;
    println!("{}", *r + x);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
