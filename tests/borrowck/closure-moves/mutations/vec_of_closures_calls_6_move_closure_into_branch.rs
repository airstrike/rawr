fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    calls.push(|| v.len());
    v.push(3);
    if __c() {
        let __m = move || __see(&calls);
        __m();
    }
    println!("{}", calls[0]());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
