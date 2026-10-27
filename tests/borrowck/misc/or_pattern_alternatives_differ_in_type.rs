fn main() {
    let pair = (Some(1), Some(String::from("a")));
    match pair {
        (Some(x), _) | (None, Some(x)) => println!("{x}"),
        _ => println!("none"),
    }
}
