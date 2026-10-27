fn main() {
    let mut x = 1;
    let r = &x;
    __see(&vec![&x].first());
    x = 2;
    println!("{}", *r + x);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
