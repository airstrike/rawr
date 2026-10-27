fn main() {
    let mut a = 1;
    let o = Some(&a);
    __see(&vec![&a].first());
    a += 1;
    println!("{o:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
