fn twice(f: &mut impl FnMut(i32)) {
    f(1);
    f(2);
}

fn bump(r: &mut Vec<i32>) {
    twice(&mut |x| r.push(x));
    r.push(3);
}

fn main() {
    let mut v = vec![0];
    bump(&mut v);
    let __b = &mut v;
    if __c() {
        __touch(__b);
    } else {
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
