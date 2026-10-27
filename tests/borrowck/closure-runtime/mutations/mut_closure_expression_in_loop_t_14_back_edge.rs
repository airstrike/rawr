fn twice(f: &mut impl FnMut(i64)) {
    f(1);
    f(2);
}

fn main() {
    let mut t = Vec::new();
    let mut u = Vec::new();
    let mut add = |k| t.push(k);
    for _ in 0..3 {
        twice(&mut add);
        twice(&mut |k| u.push(k * 10));
    }
    let __b = &mut t;
    for __i in 0..2 {
        __touch(__b);
        println!("{t:?} {u:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
