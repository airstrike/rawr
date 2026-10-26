fn main() {
    let mut v = vec![1, 2];
    let it = v.iter();
    __see(&vec![&v].first());
    v.push(3);
    println!("{}", it.count());
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
