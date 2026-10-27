fn main() {
    let values = [Some(1), None, Some(3), Some(7)];
    for value in values {
        match value {
            Some(1) | Some(3) | None => println!("small or none"),
            Some(n) => println!("other {n}"),
        }
    }
}
