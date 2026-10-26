fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    let mut __f = || __touch(&mut v);
    __f();
    v.push(3);
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
