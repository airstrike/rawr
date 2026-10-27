fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let v: Vec<&String> = vec![&a, &b];
    let x = v.iter().copied().next().unwrap();
    a.push_str("!");
    println!("{x}");
}
