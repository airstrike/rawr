fn main() {
    let mut a = 1;
    let b = 2;
    let refs = vec![&a, &b];
    println!("{refs:?}");
    a = 5;
    println!("{a}");
}
