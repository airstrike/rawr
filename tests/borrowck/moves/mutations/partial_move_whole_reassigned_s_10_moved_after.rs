struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let a = s.a;
    s = S { a, b: String::from("f") };
    println!("{} {}", s.a, s.b);
    __take(s);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
