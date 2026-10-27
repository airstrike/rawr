struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let c = move || s.a.len();
    let mut __f = || __touch(&mut s);
    let mut __g = __f;
    s.a = String::from("x");
    __g();
    let t = s;
    println!("{} {} {}", c(), t.a, t.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
