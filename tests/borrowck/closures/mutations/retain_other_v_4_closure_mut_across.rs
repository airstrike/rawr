fn main() {
    let mut v = vec![1, 2, 3];
    let limit = vec![2];
    let mut __f = || __touch(&mut v);
    v.retain(|x| *x < limit[0]);
    __f();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
