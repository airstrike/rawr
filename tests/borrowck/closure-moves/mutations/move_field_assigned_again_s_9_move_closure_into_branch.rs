struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let c = move || s.a.len();
    if __c() {
        let __m = move || __see(&s);
        __m();
    }
    s.a = String::from("x");
    let t = s;
    println!("{} {} {}", c(), t.a, t.b);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
