fn main() {
    let a = 1;
    let mut b = 2;
    let o = Some((&a, &b));
    if let Some((_, y)) = o {
        b += 1;
        println!("{y}");
    }
}
