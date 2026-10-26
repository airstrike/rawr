fn main() {
    let mut v = vec![1, 2];
    let first = |x: &Vec<i32>| x[0];
    let n = first(&v);
    let __g = || __see(&v);
    v.push(n);
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
