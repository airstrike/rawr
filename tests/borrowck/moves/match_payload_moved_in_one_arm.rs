enum E {
    A(String),
    B,
}

fn main() {
    let e = E::A(String::from("ab"));
    match e {
        E::A(ref s) => println!("{}", s),
        E::B => println!("b"),
    }
    match e {
        E::A(s) => println!("{}", s),
        E::B => println!("b"),
    }
}
