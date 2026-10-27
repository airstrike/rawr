fn main() {
    let mut v = vec![1];
    let o = Some(&v);
    v.push(2);
    println!("{}", v.len());
}
