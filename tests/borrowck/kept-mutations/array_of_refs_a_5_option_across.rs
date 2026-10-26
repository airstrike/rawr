fn main() {
    let mut a = 1;
    let b = 2;
    let refs = [&a, &b];
    let __o = Some(&a);
    a += 1;
    __see(&__o);
    println!("{}", refs[1]);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
