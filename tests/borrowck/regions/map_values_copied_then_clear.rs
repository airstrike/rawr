use std::collections::BTreeMap;

fn main() {
    let a = 1;
    let mut m: BTreeMap<i32, &i32> = BTreeMap::new();
    m.insert(1, &a);
    let w: Vec<&i32> = m.values().copied().collect();
    m.clear();
    println!("{w:?}");
}
