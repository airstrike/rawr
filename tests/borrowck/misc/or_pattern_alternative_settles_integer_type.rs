fn main() {
    let pairs = [(Some(1), None), (None, Some(200u8))];
    for pair in pairs {
        match pair {
            (Some(x), _) | (None, Some(x)) => println!("{} {}", x, x.leading_zeros()),
            _ => println!("none"),
        }
    }
}
