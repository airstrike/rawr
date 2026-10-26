fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let f = refs.first();
    let __t = vec![&a].to_vec();
    __see(&__t);
    a += 1;
    println!("{f:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
