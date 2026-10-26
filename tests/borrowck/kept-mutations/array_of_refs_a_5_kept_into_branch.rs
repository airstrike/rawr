fn main() {
    let mut a = 1;
    let b = 2;
    let refs = [&a, &b];
    let __k = __keep(&a);
    a += 1;
    if __c() {
        __see(__k);
    }
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
