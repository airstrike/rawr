fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let mut best = &&0;
    for x in &v {
        best = x;
    }
    v.clear();
    println!("{best}");
}
