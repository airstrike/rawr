fn main() {
    let v = vec![1, 2];
    let r = &v;
    println!("{}", r.len());
    let c = move || v.len();
    println!("{}", c());
}
