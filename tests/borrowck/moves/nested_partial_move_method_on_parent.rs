struct Inner {
    a: String,
    b: String,
}

impl Inner {
    fn len(&self) -> usize {
        self.b.len()
    }
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
    let a = s.inner.a;
    println!("{} {} {}", a, s.inner.len(), s.c);
}
