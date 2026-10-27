struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let a = s.a;
    let b = s.b;
    s.a = b;
    let __m = s;
    s = __m;
    s.b = a;
    let t = s;
    println!("{} {}", t.a, t.b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
