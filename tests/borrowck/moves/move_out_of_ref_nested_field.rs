struct Inner {
    a: String,
    b: String,
}

struct Outer {
    inner: Inner,
    c: String,
}

fn take(s: &Outer) -> String {
    s.inner.a
}

fn main() {
    let s = Outer {
        inner: Inner { a: String::from("ab"), b: String::from("cde") },
        c: String::from("f"),
    };
    println!("{}", take(&s));
}
