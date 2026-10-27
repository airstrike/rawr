struct S {
    a: String,
    b: String,
}

fn main() {
    let mut s = S { a: String::from("ab"), b: String::from("cde") };
    for i in 0..3 {
        let a = s.a;
        println!("{}", a);
        s.a = format!("{}", i);
    }
    if __c() {
        __take(s);
    }
    println!("{}", s.b);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
