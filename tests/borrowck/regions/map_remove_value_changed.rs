use std::collections::HashMap;

fn main() {
    let k = String::from("k");
    let mut v = String::from("v");
    let mut m: HashMap<&String, &String> = HashMap::new();
    m.insert(&k, &v);
    let got = m.remove(&&String::from("k")).unwrap();
    v.push_str("!");
    println!("{got}");
}
