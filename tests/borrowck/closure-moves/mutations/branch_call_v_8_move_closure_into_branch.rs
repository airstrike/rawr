fn main() {
    let mut v = vec![1];
    let flag = v.len() > 3;
    let mut c = || v.push(2);
    if flag {
        c();
    }
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
