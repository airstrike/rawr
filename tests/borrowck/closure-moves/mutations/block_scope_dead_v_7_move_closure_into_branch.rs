fn main() {
    let mut v = vec![1];
    {
        let r = &mut v;
        r.push(2);
    }
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(3);
    println!("{:?}", v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
