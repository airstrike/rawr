struct S {
    a: Vec<i32>,
    b: Vec<i32>,
}

fn main() {
    let mut s = S { a: vec![1], b: vec![2] };
    let c = move || s.a.len();
    s.b.push(3);
    let mut __f = || __touch(&mut s);
    let mut __g = __f;
    __g();
    __f();
    println!("{} {:?}", c(), s.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
