fn main() {
    let mut v = vec![1, 2];
    let first = |x: &Vec<i32>| x[0];
    let n = first(&v);
    v.push(n);
    let __f = || __see(&v);
    let __g = __f;
    __f();
    println!("{:?}", v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
