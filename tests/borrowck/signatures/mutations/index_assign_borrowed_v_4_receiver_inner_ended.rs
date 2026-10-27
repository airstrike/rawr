fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __t = vec![&v].to_vec();
    __see(&__t);
    v[0] = 5;
    println!("{}", r[0]);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
