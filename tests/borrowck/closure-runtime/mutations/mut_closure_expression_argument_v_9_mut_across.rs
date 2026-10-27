fn twice(f: &mut impl FnMut(i32)) {
    f(1);
    f(2);
}

fn main() {
    let mut v = Vec::new();
    twice(&mut |x| v.push(x));
    let __b = &mut v;
    twice(&mut |x| v.push(x * 10));
    __touch(__b);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
