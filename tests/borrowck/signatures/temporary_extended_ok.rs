fn main() {
    let r = &String::from("a");
    let t: &str = &String::from("b");
    let u: &String = &String::from("c");
    drop(u);
    println!("{r} {t}");
}
