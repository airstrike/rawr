struct S {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn main() {
    let mut s = S { a: vec![1], b: vec![2] };
    let c = move || s.a.len();
    if __c() {
        let __m = move || __see(&s);
        __m();
    }
    s.b.push(3);
    println!("{} {:?}", c(), s.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
