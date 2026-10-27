struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    let a = s.a;
    for __i in 0..2 {
        let __m = s;
        s = __m;
    }
    s = S { a, b: String::from("f") };
    println!("{} {}", s.a, s.b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
