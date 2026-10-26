fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    let __t = vec![&refs].to_vec();
    refs.push(&a);
    __see(&__t);
    println!("{refs:?}");
    a += 1;
    println!("{a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
