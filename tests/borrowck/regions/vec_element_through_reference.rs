fn main() {
    let a = 1;
    let mut v = vec![&a];
    let r = &v;
    let x = r[0];
    v.push(&a);
    println!("{x}");
}
