fn main() {
    let mut a = 1;
    let o = Some(&a);
    a += 1;
    println!("{o:?}");
}
