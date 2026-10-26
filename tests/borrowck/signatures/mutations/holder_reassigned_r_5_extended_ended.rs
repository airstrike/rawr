fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    let __t = &__id(&r);
    __see(__t);
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
