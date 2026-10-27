struct Inner {
    a: String,
    b: String,
}

struct Outer {
    inner: Inner,
    c: String,
}

fn main() {
    let mut s = Outer {
        inner: Inner { a: String::from("ab"), b: String::from("cde") },
        c: String::from("f"),
    };
    let b = s.inner.b;
    s.inner.b = String::from("g");
    let t = s;
    println!("{} {} {} {}", t.inner.a, t.inner.b, t.c, b);
}
