fn main() {
    let v = vec![String::from("ab")];
    let n = v.iter().count();
    println!("{} {}", n, v.len());
}
