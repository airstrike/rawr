enum E {
    A(String),
    B,
}

fn main() {
    let e = E::A(String::from("ab"));
    match e {
        E::A(s) => println!("{}", s),
        E::B => println!("b"),
    }
    if let E::B = e {
        println!("b");
    }
}
