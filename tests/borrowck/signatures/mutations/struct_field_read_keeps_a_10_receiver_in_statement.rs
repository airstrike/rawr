struct View<'a> {
    value: &'a i32,
    count: i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a, count: 0 };
    let v = view.value;
    __see(&vec![&a].first());
    a += 1;
    println!("{v} {}", view.count);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
