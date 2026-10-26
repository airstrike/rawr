struct View<'a> {
    value: &'a i32,
    count: i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a, count: 0 };
    let v = view.value;
    let __t = vec![&a].first();
    a += 1;
    __see(&__t);
    println!("{v} {}", view.count);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
