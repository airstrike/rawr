fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    r.push(1);
    let c = || v.len();
    println!("{}", c());
}
