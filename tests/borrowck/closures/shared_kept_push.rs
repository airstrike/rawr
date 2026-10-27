fn main() {
    let mut v = vec![1, 2];
    let c = || v.len();
    v.push(3);
    println!("{}", c());
}
