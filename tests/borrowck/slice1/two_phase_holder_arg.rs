fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(v.len());
    println!("{:?}", v);
}
