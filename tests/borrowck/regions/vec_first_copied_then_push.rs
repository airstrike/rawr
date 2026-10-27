fn main() {
    let a = 1;
    let mut v = vec![&a];
    let x = v.first().copied().unwrap();
    v.push(&a);
    println!("{x}");
}
