fn main() {
    let mut v = vec![1];
    let r = &v;
    v.push(1);
    println!("{r:?}");
}
