fn main() {
    let pairs = [(Some(1), None), (None, Some(2)), (None, None)];
    for pair in pairs {
        match pair {
            (Some(x), _) | (None, Some(x)) => println!("{x}"),
            _ => println!("none"),
        }
    }
}
