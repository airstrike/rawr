use std::collections::HashMap;

fn main() {
    let a = 1;
    let mut m: HashMap<i32, &i32> = HashMap::new();
    m.insert(1, &a);
    let got = m.get(&1).unwrap();
    m.insert(2, &a);
    println!("{got}");
}
