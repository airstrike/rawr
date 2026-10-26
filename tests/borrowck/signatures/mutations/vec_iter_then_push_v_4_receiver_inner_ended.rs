fn main() {
    let mut v = vec![1, 2];
    let it = v.iter();
    let __t = vec![&v].to_vec();
    __see(&__t);
    v.push(3);
    println!("{}", it.count());
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
