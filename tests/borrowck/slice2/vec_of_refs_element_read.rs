fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let first = refs[0];
    a += 1;
    println!("{first}");
}
