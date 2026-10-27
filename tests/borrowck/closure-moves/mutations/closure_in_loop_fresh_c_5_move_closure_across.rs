fn main() {
    let mut v = vec![1];
    for i in 0..3 {
        let mut c = || v.push(i);
        let __m = move || __see(&c);
        c();
        __m();
        println!("{}", v.len());
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
