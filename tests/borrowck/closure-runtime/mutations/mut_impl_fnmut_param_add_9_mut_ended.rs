fn twice(f: &mut impl FnMut(i32)) {
    f(1);
    f(2);
}

fn main() {
    let mut total = 0;
    let mut add = |x| total += x;
    let __b = &mut add;
    __touch(__b);
    twice(&mut add);
    twice(&mut add);
    add(10);
    println!("{total}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
