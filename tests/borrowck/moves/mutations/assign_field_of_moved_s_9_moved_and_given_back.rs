struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let t = s;
    let __m = s;
    s = __m;
    s.a = String::from("x");
    println!("{}", t.a);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
