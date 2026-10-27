struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let b = 2;
    let mut view = View { value: &a };
    __see(&vec![&view].first());
    println!("{}", view.value);
    view = View { value: &b };
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
