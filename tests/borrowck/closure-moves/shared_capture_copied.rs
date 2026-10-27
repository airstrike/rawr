fn main() {
    let v = vec![1, 2, 3];
    let c = || v.len();
    let d = c;
    println!("{} {}", c(), d());
}
