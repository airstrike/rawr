enum E {
    A(&str),
}

fn main() {
    let e = E::A("a");
    match e {
        E::A(s) => println!("{s}"),
    }
}
