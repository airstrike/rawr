struct D {
    a: String,
}

impl Drop for D {
    fn drop(&mut self) {}
}

struct S {
    d: D,
    b: String,
}

fn main() {
    let s = S { d: D { a: String::from("ab") }, b: String::from("c") };
    let d = s.d;
    println!("{} {}", d.a, s.b);
}
