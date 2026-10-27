fn both<T: Copy>(value: T) -> ((T, i32), (T, i32)) {
    let pair = (value, 1);
    let first = pair;
    (first, pair)
}

fn main() {
    println!("{:?}", both(2u8));
}
