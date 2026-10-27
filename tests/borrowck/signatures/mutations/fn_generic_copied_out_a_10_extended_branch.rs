fn get<T: Copy>(x: &T) -> T {
    *x
}

fn main() {
    let mut a = 1;
    let b = 2;
    let r = &b;
    let copied = get(&r);
    let __t = if __c() { &__id(&a) } else { &__id(&a) };
    a += 1;
    __see(__t);
    println!("{a} {copied}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
