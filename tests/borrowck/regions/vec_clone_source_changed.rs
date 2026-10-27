fn main() {
    let mut a = 1;
    let v = vec![&a];
    let w = v.clone();
    a += 1;
    println!("{w:?}");
}
