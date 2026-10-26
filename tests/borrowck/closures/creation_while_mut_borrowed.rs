fn main() {
    let mut v = vec![1, 2];
    let r = &mut v;
    let c = || v.len();
    r.push(1);
    println!("{}", c());
}
