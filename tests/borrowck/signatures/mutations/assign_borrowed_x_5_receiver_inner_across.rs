fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __t = vec![&x].to_vec();
    println!("{}", *r + x);
    __see(&__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
