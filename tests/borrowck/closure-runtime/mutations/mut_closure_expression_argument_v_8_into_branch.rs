fn twice(f: &mut impl FnMut(i32)) {
    f(1);
    f(2);
}

fn main() {
    let mut v = Vec::new();
    let __b = &mut v;
    twice(&mut |x| v.push(x));
    if __c() {
        __touch(__b);
    }
    twice(&mut |x| v.push(x * 10));
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
