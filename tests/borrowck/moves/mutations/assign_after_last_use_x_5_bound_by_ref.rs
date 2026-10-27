fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let ref __v = x;
    x = 2;
    println!("{}", x + y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
