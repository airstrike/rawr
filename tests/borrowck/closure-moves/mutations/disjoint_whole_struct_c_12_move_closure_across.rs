struct S {
    a: Vec<i32>,
    b: Vec<i32>,
}
fn show(s: &S) -> usize {
    s.a.len() + s.b.len()
}
fn main() {
    let mut s = S { a: vec![1], b: vec![2] };
    let mut c = || s.a.push(5);
    println!("{}", show(&s));
    let __m = move || __see(&c);
    c();
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
