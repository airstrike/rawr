fn get<T: Copy>(x: &T) -> T {
    *x
}

fn main() {
    let mut a = 1;
    let b = 2;
    let r = &b;
    let copied = get(&r);
    let __h = __Hold(&a);
    a += 1;
    __see(&__h);
    println!("{a} {copied}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
