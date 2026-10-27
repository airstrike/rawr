fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    let __t = if __c() { &__id(&x) } else { &__id(&x) };
    println!("{} {}", x, y);
    __see(__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
