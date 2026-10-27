fn main() {
    let o = Some(String::from("ab"));
    let p = Some(String::from("c"));
    let less = o < p;
    println!("{} {:?} {:?}", less, o, p);
}
