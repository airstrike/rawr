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
    let __h = || &s;
    let __k = __h();
    s.b.push(3);
    __see(__k);
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
