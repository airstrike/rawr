fn twice<T>(value: T) -> (T, T) {
    let pair = (value, 1);
    let first = pair;
    (first.0, pair.0)
}

fn main() {
    println!("{:?}", twice(3));
}
