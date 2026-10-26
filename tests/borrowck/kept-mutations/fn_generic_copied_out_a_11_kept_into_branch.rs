fn get<T: Copy>(x: &T) -> T {
    *x
}

fn main() {
    let mut a = 1;
    let b = 2;
    let r = &b;
    let copied = get(&r);
    a += 1;
    let __k = __keep(&a);
    println!("{a} {copied}");
    if __c() {
        __see(__k);
    }
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
