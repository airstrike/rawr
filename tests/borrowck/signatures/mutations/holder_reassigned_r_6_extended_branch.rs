fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    println!("{}", r.len());
    let __t = if __c() { &__id(&r) } else { &__id(&r) };
    r = &b;
    __see(__t);
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
