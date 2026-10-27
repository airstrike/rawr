fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let c = || r.len();
    v.push(1);
    println!("{}", c());
}
