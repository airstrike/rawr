fn main() {
    let a = 1;
    let mut b = 2;
    let t = (&a, &b);
    let y = t.1;
    b += 1;
    println!("{y} {b}");
}
