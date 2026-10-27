fn main() {
    let name = String::from("ab");
    let greet = move || name.len();
    let pair = (greet, 1);
    let first = pair;
    let second = pair;
    println!("{} {}", (first.0)() + first.1, (second.0)());
}
