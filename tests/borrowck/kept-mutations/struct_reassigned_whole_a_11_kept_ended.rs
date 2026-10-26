struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let b = 2;
    let mut view = View { value: &a };
    println!("{}", view.value);
    view = View { value: &b };
    let __k = __keep(&a);
    __see(__k);
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
