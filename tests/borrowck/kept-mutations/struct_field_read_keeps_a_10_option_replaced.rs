struct View<'a> {
    value: &'a i32,
    count: i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a, count: 0 };
    let v = view.value;
    let mut __o = Some(&a);
    __see(&__o);
    __o = None;
    a += 1;
    __see(&__o);
    println!("{v} {}", view.count);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
