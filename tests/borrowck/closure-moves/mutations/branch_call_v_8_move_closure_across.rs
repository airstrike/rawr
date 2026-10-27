fn main() {
    let mut v = vec![1];
    let flag = v.len() > 3;
    let mut c = || v.push(2);
    if flag {
        c();
    }
    let __m = move || __see(&v);
    println!("{:?}", v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
