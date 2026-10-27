fn main() {
    let v = vec![1];
    let c = || v.len();
    println!("{}", c());
    let w = v;
    println!("{:?}", w);
}
