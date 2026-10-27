struct D {
    a: String,
}

impl Drop for D {
    fn drop(&mut self) {}
}

struct S {
    d: D,
}

fn main() {
    let s = S { d: D { a: String::from("ab") } };
    let a = s.d.a;
    println!("{}", a);
}
