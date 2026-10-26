fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    let __h = || &calls;
    let __k = __h();
    calls.push(|| v.len());
    __see(__k);
    v.push(3);
    println!("{}", calls[0]());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
