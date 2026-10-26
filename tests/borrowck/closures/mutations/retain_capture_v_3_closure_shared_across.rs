fn main() {
    let mut v = vec![1, 2, 3];
    let __g = || __see(&v);
    v.retain(|x| *x < v.len() as i32);
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
