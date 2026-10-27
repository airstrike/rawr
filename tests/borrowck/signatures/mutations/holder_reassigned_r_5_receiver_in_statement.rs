fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    __see(&vec![&r].first());
    println!("{}", r.len());
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
