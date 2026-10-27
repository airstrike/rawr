fn twice<T>(value: T) -> (T, T) {
    let first = value;
    (first, value)
}

fn main() {
    println!("{:?}", twice(3));
}
