fn main() {
    let p = (Some(1), None);
    match p {
        (Some(x), _) | (None, Some(x)) => println!("{x}"),
        _ => println!("none"),
    }
}
