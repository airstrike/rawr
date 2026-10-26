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
    let __f = || __see(&s);
    let __g = __f;
    __f();
    s.b.push(3);
    __g();
    println!("{}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
