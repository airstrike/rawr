fn main() {
    let mut v = vec![1, 2, 3];
    let limit = vec![2];
    v.retain(|x| *x < limit[0]);
    println!("{:?}", v);
}
