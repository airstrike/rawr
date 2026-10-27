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
    let b = s.inner.b;
    println!("{} {} {}", s.inner.a, s.c, b);
}
