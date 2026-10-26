fn main() {
    let mut a = 1;
    let o = Some(&a);
    let __t = vec![&a].to_vec();
    __see(&__t);
    a += 1;
    println!("{o:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
