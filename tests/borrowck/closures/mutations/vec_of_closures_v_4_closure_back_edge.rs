fn main() {
    let mut v = vec![1, 2];
    let mut calls = Vec::new();
    let mut __f = || __touch(&mut v);
    for __i in 0..2 {
        __f();
        calls.push(|| v.len());
    }
    v.push(3);
    println!("{}", calls[0]());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
