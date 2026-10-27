fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    __take(x);
    println!("{}", x + y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
