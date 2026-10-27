fn main() {
    let a = 1;
    let mut v = vec![&a];
    let x = v.iter().next().unwrap();
    println!("{x}");
    v.push(&a);
    println!("{}", v.len());
}
