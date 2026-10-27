fn main() {
    let s = String::from("abc");
    let c = move || s.len();
    let v = vec![c];
    println!("{} {}", v[0](), c());
}
