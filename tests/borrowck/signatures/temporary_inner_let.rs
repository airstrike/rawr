fn main() {
    let mut r = &String::new();
    {
        let t = &String::from("a");
        r = t;
    }
    println!("{r}");
}
