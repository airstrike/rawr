struct S {
    a: String,
    b: String,
}

fn main() {
    let v = vec![S { a: String::from("ab"), b: String::from("c") }];
    let s = v[0].a;
    println!("{}", s);
}
