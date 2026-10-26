fn main() {
    let mut v = vec![10, 20];
    let x = v.get(1).unwrap();
    v.clear();
    println!("{x}");
}
