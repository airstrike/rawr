fn keep(s: &String) -> &String {
    s
}

fn main() {
    let r = keep(&String::from("a"));
    println!("{r}");
}
