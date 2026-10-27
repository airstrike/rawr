fn main() {
    let v = vec![String::from("ab")];
    let n = v.into_iter().count();
    println!("{} {}", n, v.len());
}
