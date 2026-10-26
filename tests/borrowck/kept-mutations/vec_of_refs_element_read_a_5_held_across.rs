fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let first = refs[0];
    let __h = __Hold(&a);
    a += 1;
    __see(&__h);
    println!("{first}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
