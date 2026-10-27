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
    if s.c.len() > 0 {
        let b = s.inner.b;
        println!("{}", b);
    }
    println!("{}", s.inner.b);
}
