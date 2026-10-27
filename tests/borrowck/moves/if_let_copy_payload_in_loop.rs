fn main() {
    let maybe = Some(3);
    for _ in 0..2 {
        if let Some(x) = maybe {
            println!("{}", x);
        }
    }
}
