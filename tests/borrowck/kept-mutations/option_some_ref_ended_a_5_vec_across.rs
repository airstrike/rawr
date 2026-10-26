fn main() {
    let mut a = 1;
    let o = Some(&a);
    println!("{o:?}");
    let __v = vec![&a];
    a += 1;
    __see(&__v);
    println!("{a}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
