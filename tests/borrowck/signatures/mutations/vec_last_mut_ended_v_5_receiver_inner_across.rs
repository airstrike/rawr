fn main() {
    let mut v = vec![1, 2, 3];
    let last = v.last_mut().unwrap();
    *last = 9;
    let __t = vec![&v].to_vec();
    println!("{} {:?}", v.len(), v);
    __see(&__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
