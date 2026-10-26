struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let b = 2;
    let mut view = View { value: &a };
    let __k = __keep(&view);
    println!("{}", view.value);
    if __c() {
        __see(__k);
    }
    view = View { value: &b };
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
