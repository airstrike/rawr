fn main() {
    let mut v = vec![1, 2];
    let first = |x: &Vec<i32>| x[0];
    let n = first(&v);
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        v.push(n);
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
