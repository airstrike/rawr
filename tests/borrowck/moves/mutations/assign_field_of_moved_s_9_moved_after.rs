struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let t = s;
    s.a = String::from("x");
    __take(s);
    println!("{}", t.a);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
