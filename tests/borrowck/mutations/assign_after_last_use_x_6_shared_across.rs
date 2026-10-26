fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __s = &x;
    println!("{}", x + y);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
