fn main() {
    let mut r = &String::new();
    println!("{r}");
    {
        let t = &String::from("a");
        r = t;
        println!("{r}");
    }
}
