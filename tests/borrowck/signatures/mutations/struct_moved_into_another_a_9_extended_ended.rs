struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a };
    let moved = view;
    let __t = &__id(&a);
    __see(__t);
    a += 1;
    println!("{}", moved.value);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
