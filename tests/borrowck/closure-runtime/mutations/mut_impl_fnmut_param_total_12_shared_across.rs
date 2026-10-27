fn twice(f: &mut impl FnMut(i32)) {
    f(1);
    f(2);
}

fn main() {
    let mut total = 0;
    let mut add = |x| total += x;
    twice(&mut add);
    twice(&mut add);
    add(10);
    let __s = &total;
    println!("{total}");
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
