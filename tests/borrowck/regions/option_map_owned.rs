fn main() {
    let mut a = String::from("a");
    let o = Some(&a);
    let n = o.map(|s| s.len());
    a.push_str("!");
    println!("{n:?} {a}");
}
