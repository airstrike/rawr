struct S {
    a: String,
    b: String,
}

fn main() {
    let s = S { a: String::from("ab"), b: String::from("cde") };
    if s.b.len() > 1 {
        let a = s.a;
        println!("{}", a);
    } else {
        println!("{}", s.a);
    }
    println!("{}", s.b);
}
