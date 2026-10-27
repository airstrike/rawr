fn main() {
    let mut v = vec![1];
    let r = &v;
    let __t = vec![&v].to_vec();
    v.push(1);
    __see(&__t);
    println!("{r:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
