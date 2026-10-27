fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let (_, y) = t;
    b += 1;
    println!("{y}");
}
