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
    let c = || s.inner.a;
    println!("{} {}", s.inner.b, s.c);
    println!("{}", c());
}
