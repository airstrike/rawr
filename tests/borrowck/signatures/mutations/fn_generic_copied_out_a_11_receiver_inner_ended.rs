fn get<T: Copy>(x: &T) -> T {
    *x
}

fn main() {
    let mut a = 1;
    let b = 2;
    let r = &b;
    let copied = get(&r);
    a += 1;
    let __t = vec![&a].to_vec();
    __see(&__t);
    println!("{a} {copied}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
