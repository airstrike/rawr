fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    println!("{} {}", pair.0, pair.1);
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
