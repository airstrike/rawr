struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let b = 2;
    let mut view = View { value: &a };
    println!("{}", view.value);
    let mut __o = Some(&view);
    __see(&__o);
    __o = None;
    view = View { value: &b };
    __see(&__o);
    a += 1;
    println!("{} {a}", view.value);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
