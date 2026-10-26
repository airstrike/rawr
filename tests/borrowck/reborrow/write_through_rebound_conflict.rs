fn main() {
    let mut x = 1;
    let mut y = 10;
    let mut r = &mut x;
    *r += 1;
    r = &mut y;
    let seen = y;
    *r *= 3;
    println!("{x} {y} {seen}");
}
