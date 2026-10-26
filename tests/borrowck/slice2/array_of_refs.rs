fn main() {
    let mut a = 1;
    let b = 2;
    let refs = [&a, &b];
    a += 1;
    println!("{}", refs[1]);
}
