fn main() {
    let t = (3, String::from("cde"));
    let n = t.0;
    let u = t;
    println!("{} {}", n, u.1);
}
