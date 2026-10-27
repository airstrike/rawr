fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    let mut __f = || __touch(&mut v);
    calls.push(|| v.len());
    if __c() {
        __f();
    }
    v.push(3);
    println!("{}", calls[0]());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
