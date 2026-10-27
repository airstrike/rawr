fn keep(s: &String) -> &String {
    s
}

fn main() {
    let n = keep(&String::from("a")).len();
    let owned = String::from("b");
    let r = keep(&owned);
    println!("{n} {r}");
}
