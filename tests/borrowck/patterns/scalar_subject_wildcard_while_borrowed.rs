fn main() {
    let mut x = 1;
    let r = &mut x;
    let n = match x {
        _ => 20,
    };
    *r += 1;
    println!("{} {}", n, x);
}
