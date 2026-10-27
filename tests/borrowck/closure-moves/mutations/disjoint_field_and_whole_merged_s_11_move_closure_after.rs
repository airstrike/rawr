struct S {
    a: Vec<i32>,
    b: Vec<i32>,
}
fn main() {
    let mut s = S { a: vec![1], b: vec![2] };
    let c = || {
        let n = s.a.len();
        n + s.b.len()
    };
    s.b.push(3);
    let __m = move || __see(&s);
    __m();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
