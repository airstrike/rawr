fn main() {
    let a = 1;
    let mut v = vec![&a];
    let x = v.pop().unwrap();
    v.push(&a);
    println!("{x}");
}
