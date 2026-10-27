fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    let d = || c() + 1;
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    v.push(1);
    __g();
    println!("{}", d());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
