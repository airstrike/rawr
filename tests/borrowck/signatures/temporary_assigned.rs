fn main() {
    let mut r = &String::from("x");
    println!("{r}");
    r = &String::from("a");
    println!("{r}");
}
