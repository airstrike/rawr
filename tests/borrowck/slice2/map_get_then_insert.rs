use std::collections::HashMap;

fn main() {
    let mut m = HashMap::new();
    m.insert(1, String::from("one"));
    let one = m.get(&1);
    m.insert(2, String::from("two"));
    println!("{one:?}");
}
