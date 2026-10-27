fn main() {
    let n = String::from("ab").as_str().len();
    let owned = String::from("c");
    let s = owned.as_str();
    println!("{n} {s}");
}
