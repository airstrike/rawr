fn main() {
    let mut a = 1;
    let b = 2;
    let refs = vec![&a, &b];
    __see(&vec![&a].first());
    a = 5;
    println!("{refs:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
