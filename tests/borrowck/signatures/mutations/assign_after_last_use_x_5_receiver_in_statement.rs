fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    __see(&vec![&x].first());
    x = 2;
    println!("{}", x + y);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
