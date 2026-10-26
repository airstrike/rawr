struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a };
    let moved = view;
    let __k = __keep(&a);
    for __i in 0..2 {
        __see(__k);
        a += 1;
    }
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
