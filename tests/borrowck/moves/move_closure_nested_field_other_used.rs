struct Inner {
    a: String,
    b: String,
}

struct Outer {
    inner: Inner,
}

fn main() {
    let s = Outer {
        inner: Inner { a: String::from("ab"), b: String::from("cde") },
    };
    let c = move || s.inner.a.len();
    println!("{} {}", s.inner.b, c());
}
