fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    __see(&vec![&a].first());
    a += 1;
    println!("{}", pair.1);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
