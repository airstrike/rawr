struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a };
    let moved = view;
    let mut __o = Some(&a);
    __see(&__o);
    __o = None;
    a += 1;
    __see(&__o);
    println!("{}", moved.value);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
