enum E {
    A(String),
    B,
}

fn main() {
    let e = E::A(String::from("ab"));
    match e {
        E::A(_) => println!("a"),
        E::B => println!("b"),
    }
    if let E::A(s) = &e {
        println!("{}", s);
    }
}
