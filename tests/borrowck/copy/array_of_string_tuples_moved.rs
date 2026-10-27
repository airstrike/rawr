fn main() {
    let names = [("a", String::from("x")), ("b", String::from("y"))];
    let first: Vec<String> = names.into_iter().map(|(_, name)| name).collect();
    let second: Vec<&str> = names.into_iter().map(|(key, _)| key).collect();
    println!("{first:?} {second:?}");
}
