fn main() {
    let mut v = vec![1, 2, 3];
    let last = v.last_mut().unwrap();
    __see(&vec![&v].first());
    println!("{}", v.len());
    *last = 9;
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
