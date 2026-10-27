fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    let __t = vec![&a].to_vec();
    __see(&__t);
    refs.push(&a);
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
