fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    v.push(r.len());
    println!("{:?}", v);
}
