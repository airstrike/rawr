fn shout(x: String) -> String {
    x.to_uppercase()
}

fn main() {
    let p = String::from("hi");
    let s = format!("{} {}", &p, shout(p));
    println!("{s}");
}
