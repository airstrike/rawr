fn main() {
    let maybe = Some(String::from("ab"));
    for _ in 0..2 {
        if let Some(x) = maybe {
            println!("{}", x);
        }
    }
}
