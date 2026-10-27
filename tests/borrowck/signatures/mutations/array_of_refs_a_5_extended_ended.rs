fn main() {
    let mut a = 1;
    let b = 2;
    let refs = [&a, &b];
    let __t = &__id(&a);
    __see(__t);
    a += 1;
    println!("{}", refs[1]);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
