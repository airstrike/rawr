fn main() {
    let v = vec![1, 2];
    let a = || v.len();
    let b = || v[0];
    println!("{} {}", a(), b());
}
