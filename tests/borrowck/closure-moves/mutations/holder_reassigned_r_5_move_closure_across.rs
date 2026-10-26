fn main() {
    let mut a = vec![1];
    let b = vec![2];
    let mut r = &a;
    let __m = move || __see(&r);
    println!("{}", r.len());
    __m();
    r = &b;
    a.push(3);
    println!("{} {:?}", r.len(), a);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
