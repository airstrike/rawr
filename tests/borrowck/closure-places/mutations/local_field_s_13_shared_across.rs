struct S {
    a: Vec<i32>,
    b: i32,
}

fn main() {
    let mut s = S { a: vec![], b: 1 };
    let r = &mut s.a;
    let mut f = |x| r.push(x);
    f(1);
    f(2);
    s.b += 1;
    let __s = &s;
    println!("{:?} {}", s.a, s.b);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
