fn main() {
    let a = 1;
    let mut v = vec![&a];
    let w = v.clone();
    v.push(&a);
    println!("{w:?}");
}
