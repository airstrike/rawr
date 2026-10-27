fn main() {
    let mut v = vec![1, 2];
    let __t = vec![&v].to_vec();
    __see(&__t);
    v[v.len() - 1] = 5;
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
