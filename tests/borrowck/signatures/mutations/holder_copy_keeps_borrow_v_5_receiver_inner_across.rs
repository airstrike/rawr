fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __t = vec![&v].to_vec();
    v.push(2);
    __see(&__t);
    println!("{}", s.len());
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
