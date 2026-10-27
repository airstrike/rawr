fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let (x, _) = t;
    b += 1;
    println!("{x} {b}");
}
