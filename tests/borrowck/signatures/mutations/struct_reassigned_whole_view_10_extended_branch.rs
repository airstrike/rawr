struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let b = 2;
    let mut view = View { value: &a };
    println!("{}", view.value);
    let __t = if __c() { &__id(&view) } else { &__id(&view) };
    view = View { value: &b };
    __see(__t);
    a += 1;
    println!("{} {a}", view.value);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
