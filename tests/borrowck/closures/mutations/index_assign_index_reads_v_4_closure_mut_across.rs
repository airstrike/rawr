fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let mut __f = || __touch(&mut v);
    println!("{:?}", v);
    __f();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
