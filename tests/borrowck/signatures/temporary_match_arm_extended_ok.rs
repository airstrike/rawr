fn main() {
    let c = std::env::args().count() > 5;
    let r = if c { &String::from("a") } else { &String::from("b") };
    let m = match c {
        true => &String::from("c"),
        false => &String::from("d"),
    };
    println!("{r} {m}");
}
