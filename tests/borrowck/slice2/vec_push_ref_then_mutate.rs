fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    a += 1;
    println!("{refs:?}");
}
