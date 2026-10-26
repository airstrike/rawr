fn main() {
    let mut a = 1;
    let o = Some(&a);
    let r = o.unwrap();
    a += 1;
    println!("{r}");
}
