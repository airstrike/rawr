fn main() {
    let mut a = 1;
    let o = Some(&a);
    println!("{o:?}");
    let __t = vec![&a].to_vec();
    a += 1;
    __see(&__t);
    println!("{a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
