fn main() {
    let mut x = 1;
    let r = &x;
    let __t = vec![&x].to_vec();
    __see(&__t);
    x = 2;
    println!("{}", *r + x);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
