fn main() {
    let v = vec![1, 2];
    let r = &v;
    let c = move || v.len();
    println!("{} {}", r.len(), c());
}
