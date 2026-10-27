fn main() {
    let a = 1;
    let mut b = 2;
    let o = Some(&a);
    let r = o.unwrap_or_else(|| &b);
    b += 1;
    println!("{r}");
}
