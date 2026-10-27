fn main() {
    let a = 1;
    let mut v = vec![&a];
    if let Some(x) = v.iter().copied().next() {
        v.push(&a);
        println!("{x}");
    }
}
