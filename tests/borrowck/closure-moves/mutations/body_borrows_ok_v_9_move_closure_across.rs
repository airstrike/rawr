fn main() {
    let mut v = vec![1, 2];
    let mut c = || {
        let r = &mut v;
        r.push(1);
        v.len()
    };
    println!("{}", c());
    let __m = move || __see(&v);
    println!("{:?}", v);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
