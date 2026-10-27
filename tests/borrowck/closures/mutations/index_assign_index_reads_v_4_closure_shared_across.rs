fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    let __g = || __see(&v);
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
