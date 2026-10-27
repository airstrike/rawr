fn main() {
    let mut v = vec![1, 2];
    let first = |x: &Vec<i32>| x[0];
    let n = first(&v);
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    v.push(n);
    __g();
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
