fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut v: Vec<&String> = vec![&a, &b];
    let x: String = v.iter().copied().next().unwrap().clone();
    v.push(&a);
    println!("{x} {}", v.len());
}
