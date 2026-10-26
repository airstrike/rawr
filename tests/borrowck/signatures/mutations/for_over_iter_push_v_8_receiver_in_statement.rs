fn main() {
    let mut v = vec![1, 2];
    for x in v.iter() {
        if *x > 5 {
            v.push(*x);
        }
    }
    __see(&vec![&v].first());
    println!("{v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
