fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    __take(x);
    x = 2;
    println!("{}", x + y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
