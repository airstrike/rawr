struct Inner {
    a: String,
    b: String,
}

struct Outer {
    inner: Inner,
    c: String,
}

fn main() {
    let s = Outer {
        inner: Inner { a: String::from("ab"), b: String::from("cde") },
        c: String::from("f"),
    };
    let i = s.inner;
    println!("{} {}", s.inner.a, i.b);
}
