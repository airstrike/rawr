fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    v.push(2);
    println!("{}", s.len());
}
