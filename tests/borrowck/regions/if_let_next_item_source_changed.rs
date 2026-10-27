fn main() {
    let mut a = 1;
    let v = vec![&a];
    if let Some(x) = v.iter().copied().next() {
        a += 1;
        println!("{x}");
    }
}
