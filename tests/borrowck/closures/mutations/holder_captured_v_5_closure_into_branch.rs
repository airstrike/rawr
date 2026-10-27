fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    let mut __f = || __touch(&mut v);
    v.push(1);
    if __c() {
        __f();
    }
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
