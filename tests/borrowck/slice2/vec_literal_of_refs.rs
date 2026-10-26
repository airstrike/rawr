fn main() {
    let mut a = 1;
    let b = 2;
    let refs = vec![&a, &b];
    a = 5;
    println!("{refs:?}");
}
