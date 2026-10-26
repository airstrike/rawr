fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    let __t = if __c() { &__id(&r) } else { &__id(&r) };
    println!("{}", r.len());
    __see(__t);
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
