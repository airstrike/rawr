// Borrow to look, clone to keep, move to give away.
fn shout(s: &str) -> String {
    s.to_uppercase()
}

fn keep(mut list: Vec<String>, item: String) -> Vec<String> {
    list.push(item);
    list
}

fn main() {
    let name = String::from("ferris");
    println!("{}", shout(&name)); // borrowed: `name` is still ours

    let copy = name.clone();
    let list = keep(Vec::new(), name); // moved: `name` is gone now
    let list = keep(list, copy);
    println!("{list:?}");

    let first: &str = &list[0];
    let again = first; // a `&str` is Copy
    println!("{first} and {again}");
}
