fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    a += 1;
    println!("{}", pair.1);
}
