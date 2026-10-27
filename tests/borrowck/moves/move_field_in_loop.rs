struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    for _ in 0..3 {
        let a = s.a;
        println!("{}", a);
    }
}
