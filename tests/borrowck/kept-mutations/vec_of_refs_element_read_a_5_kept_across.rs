fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let first = refs[0];
    let __k = __keep(&a);
    a += 1;
    __see(__k);
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
