fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    let __m = move || __see(&calls);
    calls.push(|| v.len());
    __m();
    v.push(3);
    println!("{}", calls[0]());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
