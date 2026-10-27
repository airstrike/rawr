fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = vec![&a, &b];
    let x = v.iter().copied().next().unwrap();
    v.push(&b);
    println!("{x} {}", v.len());
}
