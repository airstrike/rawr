struct Inner {
    a: String,
    b: String,
}

struct S {
    inner: Inner,
}

fn main() {
    let s = S {
        inner: Inner { a: String::from("ab"), b: String::from("cde") },
    };
    let c = move || s.inner.a.len();
    let b = s.inner.b;
    println!("{} {}", c(), b);
}
