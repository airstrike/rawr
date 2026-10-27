fn shout(x: String) -> String {
    x.to_uppercase()
}

fn main() {
    let p = String::from("hi");
    let s = format!("{} {}", shout(p.clone()), p);
    println!("{s}");
    let t = format!("{}", p);
    println!("{t} {}", shout(p));
}
