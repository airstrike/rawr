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
    let __b = &mut u;
    if __c() {
        __touch(__b);
    } else {
        println!("{t:?} {u:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
