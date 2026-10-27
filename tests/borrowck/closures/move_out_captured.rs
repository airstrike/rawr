fn main() {
    let v = vec![1];
    let c = || v.len();
    let w = v;
    println!("{} {:?}", c(), w);
}
