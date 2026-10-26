fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    let __t = vec![&a].to_vec();
    __see(&__t);
    a += 1;
    println!("{}", pair.1);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
