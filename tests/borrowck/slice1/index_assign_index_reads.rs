fn main() {
    let mut v = vec![1, 2];
    v[v.len() - 1] = 5;
    println!("{:?}", v);
}
